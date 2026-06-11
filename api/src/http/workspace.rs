//! Workspace container control: restart and full recreate, plus the agent's
//! hard reset.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use super::ApiResult;
use crate::state::AppState;

/// `POST /api/v1/workspace/restart` - restart the agent container in place.
pub async fn restart(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    info!("restarting workspace container");
    state.workspace.restart().await?;
    Ok(Json(json!({ "status": "restarted" })))
}

/// `POST /api/v1/workspace/recreate` - recreate the container, then fully
/// reprovision it (config repo, environment setup, all repos). The persistent
/// `/workspace` volume (repos + Claude session) is preserved, so the
/// conversation continues afterward.
pub async fn recreate(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    info!("recreating workspace container");
    state.workspace.recreate().await?;
    crate::orchestrator::provision_workspace(&state).await?;
    Ok(Json(json!({ "status": "recreated" })))
}

/// `POST /api/v1/workspace/provision` - reprovision in place (no recreate):
/// refresh the config repo, all repos, instruction files, and setup scripts.
pub async fn provision(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    info!("provisioning workspace");
    crate::orchestrator::provision_workspace(&state).await?;
    Ok(Json(json!({ "status": "provisioned" })))
}

#[derive(Debug, Default, Deserialize)]
pub struct ResetRequest {
    /// Also delete the agent's accumulated memory files. Off by default.
    #[serde(default)]
    pub purge_memories: bool,
}

/// `POST /api/v1/agent/reset` - hard-reset the agent: stop the current turn, wipe
/// its history and session, requeue the in-progress task, and optionally purge
/// memories. The next turn spawns a brand-new, context-free Claude session.
pub async fn reset(
    State(state): State<AppState>,
    Json(body): Json<ResetRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    crate::orchestrator::hard_reset(&state, body.purge_memories).await?;
    Ok(Json(json!({ "status": "reset" })))
}
