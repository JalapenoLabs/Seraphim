//! Lifecycle of the Claude subscription OAuth token.
//!
//! A subscription login yields an OAuth access token (`sk-ant-oat01-...`, the same
//! thing `claude setup-token` returns) that the agent runs on, plus a long-lived
//! refresh token. The access token is short-lived (~8h), so on its own the agent
//! would stop working after a few hours, or after Seraphim is offline past the
//! expiry. [`fresh_inference_token`] keeps it alive: before each turn (and on a
//! background cadence) it refreshes the access token with the refresh token when
//! the access token is expired or near expiry, persisting the rotated pair. As
//! long as the refresh token is still valid, the agent recovers automatically even
//! after days offline; only an expired or revoked refresh token forces a reconnect.
//!
//! The same loop best-effort polls `/api/oauth/usage` for the subscription usage
//! gauge, but only when the consent granted `user:profile`. The subscription
//! consent grants `user:inference` only, so in practice the gauge is skipped (the
//! endpoint would 403); the usage-limit auto-pause does not depend on it (it reads
//! the agent's own `rate_limit_event` stream).

use std::time::Duration;

use chrono::Utc;
use eyre::{eyre, Context, Result};
use tokio::time::sleep;
use tracing::{debug, info};

use crate::claude::oauth;
use crate::db::models::{ClaudeAuthMode, ClaudeUsageCredentials};
use crate::db::queries;
use crate::state::{AppState, SubscriptionUsage};

/// How often the token keepalive runs and the usage gauge is polled. Deliberately
/// infrequent: `/api/oauth/usage` returns 429 under even modest polling, and the
/// token only needs refreshing every several hours, so five minutes is ample.
const KEEPALIVE_POLL: Duration = Duration::from_secs(300);

/// Refresh the access token this far before its expiry. Set well above
/// [`KEEPALIVE_POLL`] so an idle token is always refreshed a tick before it lapses,
/// rather than briefly expiring between ticks.
const REFRESH_SKEW_MINUTES: i64 = 10;

/// The scope required to read `/api/oauth/usage`. The subscription consent grants
/// `user:inference` only, so the gauge is skipped unless a broader login is used.
const USAGE_SCOPE: &str = "user:profile";

/// Returns a usable Claude inference token, refreshing it first when needed.
///
/// In subscription mode, when an OAuth refresh token is stored and the access
/// token has expired (or is within [`REFRESH_SKEW_MINUTES`] of expiry), this trades
/// the refresh token for a fresh access token and persists the rotated pair, then
/// returns it. A valid token, a manually-pasted token (no refresh token), and
/// API-key mode are all returned unchanged.
///
/// # Errors
/// Returns an error if the refresh fails, which after a long downtime most likely
/// means the refresh token has expired or been revoked and the operator must
/// reconnect in Settings.
pub async fn fresh_inference_token(state: &AppState) -> Result<String> {
    let settings = queries::get_settings(&state.db).await?;
    if settings.claude_auth_mode != ClaudeAuthMode::Subscription {
        // API-key mode: the stored key is the credential, nothing to refresh.
        return Ok(queries::get_claude_token(&state.db).await?);
    }

    // Hold the refresh lock across the whole read-decide-refresh-persist sequence so
    // a turn and the keepalive loop never refresh the rotating token concurrently.
    let _guard = state.claude_token_refresh().lock().await;
    let credentials = queries::get_usage_credentials(&state.db).await?;

    // No refresh token means a manually-pasted setup-token (or no OAuth login):
    // use the stored token as-is and never attempt a refresh.
    if credentials.refresh_token.is_empty() {
        return Ok(queries::get_claude_token(&state.db).await?);
    }
    if !is_near_expiry(&credentials) && !credentials.access_token.is_empty() {
        return Ok(queries::get_claude_token(&state.db).await?);
    }

    refresh_and_store(state, &credentials.refresh_token).await
}

/// Whether the access token has expired or is within the refresh skew of expiry. A
/// missing expiry is treated as "refresh now" so a token of unknown age is renewed.
fn is_near_expiry(credentials: &ClaudeUsageCredentials) -> bool {
    credentials.expires_at.is_none_or(|expires_at| {
        expires_at <= Utc::now() + chrono::Duration::minutes(REFRESH_SKEW_MINUTES)
    })
}

/// Refreshes the access token and persists the rotated pair, returning the new
/// access token. Caller must hold the refresh lock.
async fn refresh_and_store(state: &AppState, refresh_token: &str) -> Result<String> {
    let tokens = oauth::refresh(refresh_token).await.wrap_err(
        "refreshing the Claude subscription token failed; the refresh token may be \
         expired or revoked. Reconnect the subscription in Settings.",
    )?;
    let expires_at = Utc::now() + chrono::Duration::seconds(tokens.expires_in);
    queries::set_oauth_tokens(
        &state.db,
        &tokens.access_token,
        &tokens.refresh_token,
        expires_at,
        &tokens.account_email,
    )
    .await?;
    info!(
        expires_in = tokens.expires_in,
        "refreshed the Claude subscription token"
    );
    Ok(tokens.access_token)
}

/// Keeps the subscription token fresh and polls the usage gauge, forever.
pub async fn token_loop(state: AppState) {
    loop {
        if let Err(error) = keepalive_once(&state).await {
            // A failed refresh here is surfaced loudly so a dead refresh token is
            // noticed before the next turn needs it; transient poll failures are
            // benign and just leave the last snapshot in place.
            debug!(error = %error, "Claude subscription keepalive tick failed");
        }
        sleep(KEEPALIVE_POLL).await;
    }
}

async fn keepalive_once(state: &AppState) -> Result<()> {
    let settings = queries::get_settings(&state.db).await?;
    if settings.claude_auth_mode != ClaudeAuthMode::Subscription || !settings.claude_usage_token_set
    {
        // API-key mode or no OAuth login: nothing to keep alive, and clear any stale
        // usage snapshot so the gauge does not show numbers from a prior login.
        state.set_usage(None);
        return Ok(());
    }

    // Refresh ahead of expiry so the next turn (or the first turn after a long idle)
    // never blocks on a refresh and a dead refresh token surfaces here promptly.
    let access_token = fresh_inference_token(state).await?;

    // The usage gauge needs `user:profile`, which the subscription consent does not
    // grant; polling without it just 403s. Skip cleanly rather than spam the
    // endpoint, leaving the UI to fall back to the rate-limit status.
    let credentials = queries::get_usage_credentials(&state.db).await?;
    if !credentials.scopes.contains(USAGE_SCOPE) {
        state.set_usage(None);
        return Ok(());
    }

    let usage = oauth::fetch_usage(&access_token)
        .await
        .map_err(|error| eyre!("usage poll skipped: {error}"))?;
    state.set_usage(Some(SubscriptionUsage {
        five_hour_utilization: usage.five_hour_utilization,
        five_hour_resets_at: usage.five_hour_resets_at,
        seven_day_utilization: usage.seven_day_utilization,
        seven_day_resets_at: usage.seven_day_resets_at,
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials(expires_at: Option<chrono::DateTime<Utc>>) -> ClaudeUsageCredentials {
        ClaudeUsageCredentials {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at,
            scopes: "user:inference".into(),
        }
    }

    #[test]
    fn token_well_within_its_lifetime_is_not_refreshed() {
        let later = Utc::now() + chrono::Duration::hours(4);
        assert!(!is_near_expiry(&credentials(Some(later))));
    }

    #[test]
    fn token_within_the_skew_is_refreshed() {
        let soon = Utc::now() + chrono::Duration::minutes(REFRESH_SKEW_MINUTES - 1);
        assert!(is_near_expiry(&credentials(Some(soon))));
    }

    #[test]
    fn expired_token_is_refreshed() {
        // The multi-day-offline case: expiry is long past, so we refresh on return.
        let past = Utc::now() - chrono::Duration::days(3);
        assert!(is_near_expiry(&credentials(Some(past))));
    }

    #[test]
    fn unknown_expiry_is_refreshed() {
        assert!(is_near_expiry(&credentials(None)));
    }
}
