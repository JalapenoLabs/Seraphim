//! Railway management: CRUD over the parallel agent lanes, repo assignment, the
//! per-railway pause, and manual container start/stop.
//!
//! A **railway** is a named agent lane with its own workspace container, agent
//! loop, Claude session, and set of repos (see `db::models::Railway`). The
//! undeletable `main` railway owns everything by default; the operator can create
//! more lanes here, move repos between them, pause one independently of the global
//! master pause, and start or stop a lane's container.
//!
//! The guard-bearing actions (delete, repo move, start, stop) live in the
//! orchestrator so the container and session side effects stay in one place; this
//! module is the thin REST surface that maps a rejected guard to a `400`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::Railway;
use crate::db::queries;
use crate::orchestrator::{self, RailwayActionError};
use crate::state::AppState;

/// Renders a rejected railway action as a `400` carrying its operator-facing
/// message, so the UI can explain why nothing happened. `NotFound` becomes a
/// `404` to match the rest of the REST surface.
fn reject(error: RailwayActionError) -> Response {
    let status = match error {
        RailwayActionError::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(json!({ "error": error.message() }))).into_response()
}

/// `GET /api/v1/railways` - every railway, `main` first then by swimlane rank.
pub async fn list(State(state): State<AppState>) -> ApiResult<Json<Vec<Railway>>> {
    Ok(Json(queries::list_railways(&state.db).await?))
}

/// `GET /api/v1/railways/:id` - one railway, or `404` if it does not exist.
pub async fn get(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    match queries::get_railway(&state.db, id).await? {
        Some(railway) => Ok(Json(railway).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateRailwayRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// `POST /api/v1/railways` - create a lane. It starts stopped (its container is
/// created lazily on first work), not paused, and never `main`.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateRailwayRequest>,
) -> ApiResult<Json<Railway>> {
    let railway =
        queries::create_railway(&state.db, body.name.trim(), body.description.trim()).await?;
    state.notify_board();
    Ok(Json(railway))
}

#[derive(Debug, Deserialize)]
pub struct UpdateRailwayRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// `PUT /api/v1/railways/:id` - rename a lane and edit its description.
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRailwayRequest>,
) -> ApiResult<Response> {
    match queries::update_railway(&state.db, id, body.name.trim(), body.description.trim()).await? {
        Some(railway) => {
            state.notify_board();
            Ok(Json(railway).into_response())
        }
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

/// `DELETE /api/v1/railways/:id` - delete a non-`main` lane. `main` is undeletable;
/// otherwise the lane's repos and tasks fall back to `main`, its container is torn
/// down, and its session is cleared. Blocked while a live turn runs on the lane.
pub async fn delete(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    match orchestrator::delete_railway(&state, id).await? {
        Ok(()) => Ok(Json(json!({ "deleted": true })).into_response()),
        Err(error) => Ok(reject(error)),
    }
}

#[derive(Debug, Deserialize)]
pub struct PauseRailwayRequest {
    pub paused: bool,
}

/// `POST /api/v1/railways/:id/pause` - toggle this lane's per-railway pause. This
/// is independent of the global master pause (`POST /settings/pause`); either one
/// being set stops the lane pulling new work.
pub async fn set_pause(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PauseRailwayRequest>,
) -> ApiResult<Response> {
    match queries::set_railway_paused(&state.db, id, body.paused).await? {
        Some(railway) => {
            state.notify_board();
            Ok(Json(railway).into_response())
        }
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

#[derive(Debug, Deserialize)]
pub struct AssignRepoRequest {
    pub repo_id: Uuid,
}

/// `POST /api/v1/railways/:id/repos` - move a repo (and all its tasks) onto this
/// lane. Blocked while a live turn is working the repo on its current lane;
/// otherwise the repo and its tasks move cleanly (the railway follows the repo).
pub async fn assign_repo(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AssignRepoRequest>,
) -> ApiResult<Response> {
    match orchestrator::move_repo_to_railway(&state, body.repo_id, id).await? {
        Ok(repo) => Ok(Json(repo).into_response()),
        Err(error) => Ok(reject(error)),
    }
}

/// `POST /api/v1/railways/:id/start` - manually start (and provision) this lane's
/// container. `main` is the always-on compose workspace, so this is a no-op for it
/// reported with a clear message.
pub async fn start(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    if orchestrator::start_railway(&state, id).await? {
        Ok(Json(json!({ "status": "started" })).into_response())
    } else {
        Ok(Json(json!({
            "status": "noop",
            "message": "The main railway is always running and cannot be started manually."
        }))
        .into_response())
    }
}

/// `POST /api/v1/railways/:id/stop` - manually idle-stop this lane's container,
/// keeping its clones and session for a fast restart. `main` cannot be stopped (it
/// is compose-managed); that is reported with a clear message.
pub async fn stop(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    if orchestrator::stop_railway(&state, id).await? {
        Ok(Json(json!({ "status": "stopped" })).into_response())
    } else {
        Ok(Json(json!({
            "status": "noop",
            "message": "The main railway is always running and cannot be stopped."
        }))
        .into_response())
    }
}
