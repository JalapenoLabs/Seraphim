//! Workspace container control: restart and full recreate.

use axum::extract::State;
use axum::Json;
use serde_json::json;
use tracing::info;

use super::ApiResult;
use crate::db::queries;
use crate::state::AppState;

/// `POST /api/v1/workspace/restart` - restart the agent container in place.
pub async fn restart(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    info!("restarting workspace container");
    state.workspace.restart().await?;
    Ok(Json(json!({ "status": "restarted" })))
}

/// `POST /api/v1/workspace/recreate` - recreate the container, then re-run the
/// base setup script. The persistent `/workspace` volume (repos + Claude
/// session) is preserved, so the conversation continues afterward.
pub async fn recreate(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    info!("recreating workspace container");
    state.workspace.recreate().await?;

    let settings = queries::get_settings(&state.db).await?;
    if !settings.base_setup_script.trim().is_empty() {
        let output = state
            .workspace
            .exec_capture(
                "/workspace",
                vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    settings.base_setup_script.clone(),
                ],
                Vec::new(),
            )
            .await?;
        if !output.succeeded() {
            return Ok(Json(json!({
                "status": "recreated",
                "setup_exit_code": output.exit_code,
                "setup_output": output.output,
            })));
        }
    }

    Ok(Json(json!({ "status": "recreated" })))
}
