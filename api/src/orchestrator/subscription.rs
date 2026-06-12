//! Background polling of the Claude subscription usage gauge.
//!
//! Only the usage gauge depends on this loop; the agent's inference runs on the
//! stable long-lived token and is never affected. When subscription credentials
//! are configured, the loop refreshes the short-lived access token as needed and
//! polls `/api/oauth/usage`, caching the result on [`AppState`] for the stats
//! endpoints. The endpoint rate-limits aggressively, so polling is infrequent and
//! a failed poll simply leaves the last good snapshot in place.

use std::time::Duration;

use chrono::Utc;
use eyre::{eyre, Result};
use tokio::time::sleep;
use tracing::debug;

use crate::claude::oauth;
use crate::db::models::{ClaudeAuthMode, ClaudeUsageCredentials};
use crate::db::queries;
use crate::state::{AppState, SubscriptionUsage};

/// How often the subscription usage is polled. Deliberately infrequent: the
/// `/api/oauth/usage` endpoint returns 429 under even modest polling, and the
/// number moves slowly, so a five-minute cadence keeps the gauge fresh enough
/// without tripping the limit.
const USAGE_POLL: Duration = Duration::from_secs(300);

/// Refresh the usage access token this far before its expiry, so a poll never
/// races the expiry boundary.
const REFRESH_SKEW_MINUTES: i64 = 2;

/// Polls the subscription usage forever, caching each result on [`AppState`].
pub async fn usage_loop(state: AppState) {
    loop {
        if let Err(error) = poll_once(&state).await {
            // A 429 or transient failure: keep the last snapshot and try next tick.
            debug!(error = %error, "subscription usage poll skipped");
        }
        sleep(USAGE_POLL).await;
    }
}

async fn poll_once(state: &AppState) -> Result<()> {
    let settings = queries::get_settings(&state.db).await?;
    if settings.claude_auth_mode != ClaudeAuthMode::Subscription || !settings.claude_usage_token_set
    {
        // No subscription usage credentials (API-key mode, or not logged in): make
        // sure no stale snapshot lingers, and skip the network call.
        state.set_usage(None);
        return Ok(());
    }

    let credentials = queries::get_usage_credentials(&state.db).await?;
    let access_token = ensure_fresh_access(state, &credentials).await?;
    let usage = oauth::fetch_usage(&access_token).await?;
    state.set_usage(Some(SubscriptionUsage {
        five_hour_utilization: usage.five_hour_utilization,
        five_hour_resets_at: usage.five_hour_resets_at,
        seven_day_utilization: usage.seven_day_utilization,
        seven_day_resets_at: usage.seven_day_resets_at,
    }));
    Ok(())
}

/// Returns a usable access token, refreshing and persisting it first if it is
/// expired (or within [`REFRESH_SKEW_MINUTES`] of expiry).
async fn ensure_fresh_access(
    state: &AppState,
    credentials: &ClaudeUsageCredentials,
) -> Result<String> {
    let near_expiry = credentials.expires_at.is_none_or(|expires_at| {
        expires_at <= Utc::now() + chrono::Duration::minutes(REFRESH_SKEW_MINUTES)
    });
    if !near_expiry && !credentials.access_token.is_empty() {
        return Ok(credentials.access_token.clone());
    }
    if credentials.refresh_token.is_empty() {
        return Err(eyre!("no refresh token to refresh the usage access token"));
    }

    let tokens = oauth::refresh(&credentials.refresh_token).await?;
    let expires_at = Utc::now() + chrono::Duration::seconds(tokens.expires_in);
    queries::set_usage_tokens(
        &state.db,
        &tokens.access_token,
        &tokens.refresh_token,
        expires_at,
    )
    .await?;
    Ok(tokens.access_token)
}
