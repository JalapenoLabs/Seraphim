//! Claude.ai subscription OAuth: the same Authorization-Code + PKCE flow
//! `claude auth login` uses, so an operator can connect their subscription from
//! Seraphim's UI instead of pasting a `setup-token`.
//!
//! The code exchange yields an OAuth **access token the agent runs on directly**
//! (an `sk-ant-oat01-...` token, like `claude setup-token` returns). There is no
//! `create_api_key` mint step: it requires the `org:create_api_key` scope, which
//! this login does not request, and the access token already authorizes inference.
//! The token is short-lived (~8h), so the same access/refresh pair is refreshed
//! ahead of expiry by [`crate::orchestrator::subscription`] to keep the agent
//! running, including after Seraphim has been offline for days.
//!
//! The login requests `user:profile user:inference`: `user:inference` runs the
//! agent and `user:profile` authorizes `/api/oauth/usage`, which drives the
//! subscription usage gauge. Both are accepted by the consent screen (the full
//! `claude auth login` "grant access to..." screen).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use eyre::{eyre, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

/// Claude Code's public OAuth client (the same one `claude` uses; visible in the
/// authorize URL `claude setup-token` prints).
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
// The consent host. The Claude Code CLI enters at `claude.com/cai/oauth/authorize`,
// which 302-redirects here with the query string preserved, so navigating straight
// to `claude.ai/oauth/authorize` is equivalent and what we use.
const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Beta header required on the OAuth-authenticated Anthropic endpoints.
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// OAuth scope, matched to what `claude auth login` (the interactive login, not
/// `setup-token`) requests for this client. `user:inference` runs the agent;
/// `user:profile` authorizes `/api/oauth/usage`, which drives the subscription
/// usage gauge.
///
/// Both scopes are accepted by the consent screen (it is the same "grant access
/// to..." screen `claude auth login` shows, with the full set of checkboxes). An
/// earlier revision dropped `user:profile` believing it caused the consent to
/// reject with "Invalid request format"; that rejection was actually a too-short
/// `state` (since fixed in [`start`]), not the scope. The literal
/// `user:profile user:inference` scope string is present verbatim in the Claude
/// Code binary.
const SCOPES: &str = "user:profile user:inference";

/// The PKCE secrets for one in-flight authorization, held until the operator
/// pastes the code back.
#[derive(Debug, Clone)]
pub struct PendingAuth {
    pub verifier: String,
    pub state: String,
}

/// Tokens returned by an OAuth code-exchange or refresh.
#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    /// Empty if the endpoint did not rotate the refresh token (keep the old one).
    pub refresh_token: String,
    /// Seconds until the access token expires.
    pub expires_in: i64,
    pub scopes: String,
}

/// A subscription usage snapshot (rolling-window utilization percentages).
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub five_hour_utilization: Option<f64>,
    /// Unix seconds.
    pub five_hour_resets_at: Option<i64>,
    pub seven_day_utilization: Option<f64>,
    pub seven_day_resets_at: Option<i64>,
}

/// Builds the consent URL for a new authorization and the PKCE secrets to hold
/// until the operator pastes the code back. No network.
pub fn start() -> (String, PendingAuth) {
    let verifier = url_safe(&random_bytes_32());
    let challenge = url_safe(&sha256(verifier.as_bytes()));
    // `state` must carry the same entropy the CLI uses (32 bytes -> 43 base64url
    // chars). The consent endpoint rejects a shorter value with "Authorization
    // failed"; a 16-byte UUID state was the real cause of the failed logins, even
    // once the scope and host already matched the CLI exactly.
    let state = url_safe(&random_bytes_32());
    let url = format!(
        "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code\
         &redirect_uri={redirect}&scope={scope}&code_challenge={challenge}\
         &code_challenge_method=S256&state={state}",
        redirect = percent_encode(REDIRECT_URI),
        scope = percent_encode(SCOPES),
    );
    // Log the full consent URL (it carries only ephemeral PKCE/state, no secret)
    // so an operator hitting "Invalid request format" can confirm the exact
    // scope/redirect/client the request used.
    info!(scope = SCOPES, redirect = REDIRECT_URI, %url, "built Claude OAuth authorize URL");
    (url, PendingAuth { verifier, state })
}

/// Exchanges the pasted authorization code for tokens.
///
/// The callback page shows the value as `<code>#<state>`; we accept either that
/// or a bare code.
pub async fn exchange_code(
    code_input: &str,
    verifier: &str,
    expected_state: &str,
) -> Result<Tokens> {
    let (code, state) = split_code(code_input);
    // PKCE already protects the exchange; the state echo is an extra CSRF guard
    // when the operator pastes the full `<code>#<state>` the callback page shows.
    if !expected_state.is_empty() && !state.is_empty() && state != expected_state {
        return Err(eyre!("authorization state mismatch; restart the login"));
    }
    token_request(&json!({
        "grant_type": "authorization_code",
        "client_id": CLIENT_ID,
        "code": code,
        "state": state,
        "redirect_uri": REDIRECT_URI,
        "code_verifier": verifier,
    }))
    .await
}

/// Trades a refresh token for a fresh access token (and possibly a rotated
/// refresh token).
pub async fn refresh(refresh_token: &str) -> Result<Tokens> {
    token_request(&json!({
        "grant_type": "refresh_token",
        "client_id": CLIENT_ID,
        "refresh_token": refresh_token,
    }))
    .await
}

/// Polls the subscription usage. Returns a distinct error on 429 so the caller
/// can back off (the endpoint rate-limits aggressively).
pub async fn fetch_usage(access_token: &str) -> Result<Usage> {
    let client = reqwest::Client::new();
    let response = client
        .get(USAGE_URL)
        .bearer_auth(access_token)
        .header("anthropic-beta", OAUTH_BETA)
        .send()
        .await
        .wrap_err("usage request failed")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status.as_u16() == 429 {
        return Err(eyre!("usage endpoint rate limited (429)"));
    }
    if !status.is_success() {
        return Err(eyre!("usage endpoint returned {status}: {text}"));
    }
    let value: Value = serde_json::from_str(&text)
        .wrap_err_with(|| format!("unexpected usage response: {text}"))?;
    Ok(parse_usage(&value))
}

// --- helpers -----------------------------------------------------------------

async fn token_request(body: &Value) -> Result<Tokens> {
    #[derive(Deserialize)]
    struct Response {
        access_token: String,
        #[serde(default)]
        refresh_token: String,
        #[serde(default)]
        expires_in: i64,
        #[serde(default)]
        scope: String,
    }
    let client = reqwest::Client::new();
    let response = client
        .post(TOKEN_URL)
        .header("anthropic-beta", OAUTH_BETA)
        .json(body)
        .send()
        .await
        .wrap_err("oauth token request failed")?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        // The body is the provider's error description, not a secret.
        warn!(%status, body = %text, "Claude OAuth token endpoint returned an error");
        return Err(eyre!("oauth token endpoint returned {status}: {text}"));
    }
    info!(%status, "Claude OAuth token exchange/refresh succeeded");
    let parsed: Response = serde_json::from_str(&text)
        .wrap_err_with(|| format!("unexpected token response: {text}"))?;
    Ok(Tokens {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        // Default to one hour if the endpoint omits it; the refresh loop refreshes
        // ahead of expiry regardless.
        expires_in: if parsed.expires_in > 0 {
            parsed.expires_in
        } else {
            3600
        },
        scopes: parsed.scope,
    })
}

/// Splits a pasted `<code>#<state>` (or a bare code) into its parts.
fn split_code(input: &str) -> (&str, &str) {
    let trimmed = input.trim();
    trimmed.split_once('#').unwrap_or((trimmed, ""))
}

/// Parses the usage payload, tolerating the two shapes seen in the wild: the
/// claude.ai org shape (`{five_hour:{utilization, resets_at}}`) and the
/// status-line shape (`{rate_limits:{five_hour:{used_percentage, resets_at}}}`).
fn parse_usage(value: &Value) -> Usage {
    let root = value.get("rate_limits").unwrap_or(value);
    let window = |name: &str| -> (Option<f64>, Option<i64>) {
        let Some(window) = root.get(name) else {
            return (None, None);
        };
        let utilization = window
            .get("utilization")
            .or_else(|| window.get("used_percentage"))
            .and_then(Value::as_f64);
        let resets_at = window
            .get("resets_at")
            .or_else(|| window.get("resetsAt"))
            .and_then(parse_epoch_seconds);
        (utilization, resets_at)
    };
    let (five_hour_utilization, five_hour_resets_at) = window("five_hour");
    let (seven_day_utilization, seven_day_resets_at) = window("seven_day");
    Usage {
        five_hour_utilization,
        five_hour_resets_at,
        seven_day_utilization,
        seven_day_resets_at,
    }
}

/// A reset time may arrive as unix seconds (integer) or an ISO-8601 string.
fn parse_epoch_seconds(value: &Value) -> Option<i64> {
    if let Some(seconds) = value.as_i64() {
        return Some(seconds);
    }
    let text = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|datetime| datetime.timestamp())
}

fn random_bytes_32() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes
}

fn sha256(input: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finalize().into()
}

fn url_safe(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// RFC 3986 percent-encoding of everything outside the unreserved set.
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(HEX[usize::from(byte >> 4)] as char);
                out.push(HEX[usize::from(byte & 0x0f)] as char);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_builds_a_valid_pkce_url() {
        let (url, pending) = start();
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("state={}", pending.state)));

        // The scope must request both `user:profile` (for the usage gauge) and
        // `user:inference` (to run the agent), space-separated and percent-encoded,
        // matching the `claude auth login` consent.
        assert!(url.contains("scope=user%3Aprofile%20user%3Ainference"));

        // The state must be 32 bytes of entropy (43 base64url chars), matching the
        // CLI. A shorter state makes the consent endpoint reject with
        // "Authorization failed".
        assert_eq!(pending.state.len(), 43, "state must be 32-byte base64url");

        // The challenge in the URL must be base64url(sha256(verifier)).
        let expected_challenge = url_safe(&sha256(pending.verifier.as_bytes()));
        assert!(
            url.contains(&format!("code_challenge={expected_challenge}")),
            "challenge in URL does not match the verifier"
        );
        // Two authorizations differ.
        let (_, other) = start();
        assert_ne!(pending.verifier, other.verifier);
    }

    #[test]
    fn split_code_handles_both_forms() {
        assert_eq!(split_code("abc#xyz"), ("abc", "xyz"));
        assert_eq!(split_code("  abc#xyz  "), ("abc", "xyz"));
        assert_eq!(split_code("just-a-code"), ("just-a-code", ""));
    }

    #[test]
    fn percent_encode_escapes_reserved() {
        assert_eq!(percent_encode("a b:c"), "a%20b%3Ac");
        assert_eq!(percent_encode("plain-_.~"), "plain-_.~");
        assert_eq!(
            percent_encode("org:create_api_key user:profile"),
            "org%3Acreate_api_key%20user%3Aprofile"
        );
    }

    #[test]
    fn parse_usage_handles_claude_ai_shape() {
        let value = json!({
            "five_hour": { "utilization": 34, "resets_at": "2026-06-12T06:39:59+00:00" },
            "seven_day": { "utilization": 15, "resets_at": "2026-06-18T19:59:59+00:00" },
        });
        let usage = parse_usage(&value);
        assert_eq!(usage.five_hour_utilization, Some(34.0));
        assert_eq!(usage.seven_day_utilization, Some(15.0));
        assert!(usage.five_hour_resets_at.is_some());
    }

    #[test]
    fn parse_usage_handles_statusline_shape() {
        let value = json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 34.0, "resets_at": 1_781_246_400_i64 },
                "seven_day": { "used_percentage": 15.0, "resets_at": 1_781_700_000_i64 },
            }
        });
        let usage = parse_usage(&value);
        assert_eq!(usage.five_hour_utilization, Some(34.0));
        assert_eq!(usage.five_hour_resets_at, Some(1_781_246_400));
        assert_eq!(usage.seven_day_resets_at, Some(1_781_700_000));
    }

    #[test]
    fn parse_usage_tolerates_missing_windows() {
        let usage = parse_usage(&json!({}));
        assert_eq!(usage.five_hour_utilization, None);
        assert_eq!(usage.five_hour_resets_at, None);
    }
}
