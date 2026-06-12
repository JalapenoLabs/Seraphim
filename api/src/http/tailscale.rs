//! Tailscale sidecar management endpoints (issue #52): read the node status and
//! run the handful of management actions surfaced in the Settings UI.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::info;

use super::ApiResult;
use crate::state::AppState;
use crate::tailscale::TailscaleStatus;

/// A management action's result plus the refreshed node status, so the UI updates
/// in one round-trip and can show what happened.
#[derive(Debug, Serialize)]
pub struct TailscaleActionResponse {
    /// Whether the underlying command succeeded.
    pub ok: bool,
    /// A short human-readable result (the command's own output when it has any).
    pub message: String,
    /// The node status after the action.
    pub status: TailscaleStatus,
}

/// `GET /api/v1/tailscale/status` - the node's current state for the UI panel.
pub async fn status(State(state): State<AppState>) -> ApiResult<Json<TailscaleStatus>> {
    Ok(Json(state.tailscale.status().await?))
}

/// `POST /api/v1/tailscale/up` - connect the node to the tailnet.
pub async fn up(State(state): State<AppState>) -> ApiResult<Json<TailscaleActionResponse>> {
    info!("tailscale up requested");
    let (ok, message) = match state.tailscale.up().await {
        Ok(output) => (
            output.succeeded(),
            action_message(&output.output, "Connecting to the tailnet."),
        ),
        Err(error) => (false, error.to_string()),
    };
    Ok(Json(action_response(&state, ok, message).await))
}

/// `POST /api/v1/tailscale/down` - disconnect the node from the tailnet.
pub async fn down(State(state): State<AppState>) -> ApiResult<Json<TailscaleActionResponse>> {
    info!("tailscale down requested");
    let (ok, message) = match state.tailscale.down().await {
        Ok(output) => (
            output.succeeded(),
            action_message(&output.output, "Disconnecting from the tailnet."),
        ),
        Err(error) => (false, error.to_string()),
    };
    Ok(Json(action_response(&state, ok, message).await))
}

#[derive(Debug, Default, Deserialize)]
pub struct ReauthRequest {
    /// Force a fresh login even if the node is already authenticated (this is how
    /// the operator gets a new login URL). Off by default.
    #[serde(default)]
    pub force: bool,
}

/// `POST /api/v1/tailscale/reauth` - start an interactive login and surface the
/// URL the operator must visit to authenticate the node.
pub async fn reauth(
    State(state): State<AppState>,
    Json(body): Json<ReauthRequest>,
) -> ApiResult<Json<TailscaleActionResponse>> {
    info!(force = body.force, "tailscale reauth requested");
    let (ok, message, login_url) = match state.tailscale.reauth(body.force).await {
        Ok(Some(url)) => (
            true,
            "Open the login URL to authenticate this node.".to_string(),
            Some(url),
        ),
        Ok(None) => (
            true,
            "Re-authentication started; refresh for the login URL.".to_string(),
            None,
        ),
        Err(error) => (false, error.to_string(), None),
    };
    let mut response = action_response(&state, ok, message).await;
    // Prefer the URL the daemon reports in status; otherwise the one login printed.
    if response.status.auth_url.is_none() {
        response.status.auth_url = login_url;
    }
    Ok(Json(response))
}

/// `POST /api/v1/tailscale/restart` - restart the Tailscale container in place.
pub async fn restart(State(state): State<AppState>) -> ApiResult<Json<TailscaleActionResponse>> {
    info!("tailscale restart requested");
    let (ok, message) = match state.tailscale.restart().await {
        Ok(()) => (true, "Restarting the Tailscale container.".to_string()),
        Err(error) => (false, error.to_string()),
    };
    Ok(Json(action_response(&state, ok, message).await))
}

/// Re-reads the node status (best-effort) and packages it with the action result.
async fn action_response(state: &AppState, ok: bool, message: String) -> TailscaleActionResponse {
    let status = state.tailscale.status().await.unwrap_or_default();
    TailscaleActionResponse {
        ok,
        message,
        status,
    }
}

/// The command's own output, trimmed, or a friendly fallback when it printed none.
fn action_message(output: &str, fallback: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}
