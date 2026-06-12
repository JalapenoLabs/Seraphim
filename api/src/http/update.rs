//! Self-update endpoints: report the running version, check for a newer one, and
//! trigger the rebuild. The actual work happens in [`crate::update`].

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::json;

use super::ApiResult;
use crate::db::queries;
use crate::state::{AppState, UpdateStatus};
use crate::update;

#[derive(Serialize)]
pub struct VersionResponse {
    sha: String,
    branch: String,
}

/// `GET /api/v1/version` - the commit/branch the running build came from. The UI
/// polls this after triggering an update and reloads once the SHA changes.
pub async fn version(State(state): State<AppState>) -> Json<VersionResponse> {
    Json(VersionResponse {
        sha: state.update.git_sha.clone(),
        branch: state.update.git_branch.clone(),
    })
}

#[derive(Serialize)]
pub struct UpdateStatusResponse {
    #[serde(flatten)]
    status: UpdateStatus,
    /// Whether the agent is paused (a prerequisite for updating).
    agent_paused: bool,
    /// Whether a turn is actively running (the Update button is disabled then).
    agent_working: bool,
}

async fn status_response(state: &AppState) -> ApiResult<UpdateStatusResponse> {
    let settings = queries::get_settings(&state.db).await?;
    let working = queries::any_task_in_progress(&state.db).await?;
    Ok(UpdateStatusResponse {
        status: state.update_status(),
        agent_paused: settings.agent_paused,
        agent_working: working,
    })
}

/// `GET /api/v1/update/status` - the cached check result + live agent state.
pub async fn status(State(state): State<AppState>) -> ApiResult<Json<UpdateStatusResponse>> {
    Ok(Json(status_response(&state).await?))
}

/// `POST /api/v1/update/check` - re-check for updates right now.
pub async fn check(State(state): State<AppState>) -> ApiResult<Json<UpdateStatusResponse>> {
    update::check(&state).await?;
    Ok(Json(status_response(&state).await?))
}

/// `POST /api/v1/update` - pull the latest source and rebuild the stack. Refuses
/// while the agent is mid-turn; pauses the agent before the rebuild begins.
pub async fn run(State(state): State<AppState>) -> ApiResult<Response> {
    if queries::any_task_in_progress(&state.db).await? {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "The agent is working. Wait for it to finish (or pause it) before updating."
            })),
        )
            .into_response());
    }
    if state.update.host_repo_dir.trim().is_empty() {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Self-update isn't configured: set HOST_REPO_DIR in .env." })),
        )
            .into_response());
    }

    // The agent must be paused before the rebuild begins (so nothing new starts
    // while the stack is coming down and back up).
    queries::set_paused(&state.db, true).await?;
    state.notify_board();

    update::trigger(&state).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "updating" }))).into_response())
}
