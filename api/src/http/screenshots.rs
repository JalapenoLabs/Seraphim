//! Agent screenshots (issue #248): capture, store, and serve the images the agent
//! takes during a task.
//!
//! The `seraphim-screenshot` helper inside the workspace uploads the raw image
//! bytes (the request body) plus metadata (query params). The bytes live as
//! `bytea` and are NEVER returned in a task/board payload, mirroring the
//! notification-sound precedent; a dedicated streaming endpoint serves them by id,
//! and the task view lists only the metadata (see [`super::tasks::TaskDetail`]).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::queries;
use crate::state::AppState;

/// The largest screenshot we accept. A full-page 1280px PNG sits comfortably under
/// this; the cap bounds a runaway upload while leaving real captures headroom. The
/// upload route raises axum's default 2MB body limit to this (see `http::router`).
pub const MAX_SCREENSHOT_BYTES: usize = 10 * 1024 * 1024;

/// Metadata the uploader passes as query params alongside the raw image body.
#[derive(Debug, Deserialize)]
pub struct CreateParams {
    pub task_id: Uuid,
    #[serde(default)]
    pub caption: String,
    #[serde(default)]
    pub route: String,
    /// Pixel dimensions, when the uploader could read them (e.g. a PNG header).
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// `POST /api/v1/agent/screenshots?task_id=..&caption=..&route=..&width=..&height=..`
///
/// Called from inside the workspace by `seraphim-screenshot`. The raw image is the
/// request body and its `Content-Type` becomes the stored MIME; it must be an
/// `image/*` type. The capture is best-effort associated with the task's latest
/// turn. Returns the new screenshot's id.
pub async fn create(
    State(state): State<AppState>,
    Query(params): Query<CreateParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    if queries::get_task(&state.db, params.task_id)
        .await?
        .is_none()
    {
        return Ok((StatusCode::NOT_FOUND, "task not found").into_response());
    }
    if body.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, "empty image").into_response());
    }
    if body.len() > MAX_SCREENSHOT_BYTES {
        return Ok((
            StatusCode::PAYLOAD_TOO_LARGE,
            "screenshot is too large (max 10 MB)",
        )
            .into_response());
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !mime.starts_with("image/") {
        return Ok((
            StatusCode::BAD_REQUEST,
            "body must be an image (set Content-Type: image/...)",
        )
            .into_response());
    }

    // Best-effort: tie the capture to the turn that most likely took it.
    let turn_id = queries::latest_turn_id(&state.db, params.task_id).await?;
    let screenshot = queries::create_screenshot(
        &state.db,
        params.task_id,
        turn_id,
        &body,
        mime,
        params.width,
        params.height,
        params.route.trim(),
        params.caption.trim(),
    )
    .await?;

    Ok((StatusCode::CREATED, Json(json!({ "id": screenshot.id }))).into_response())
}

/// `GET /api/v1/screenshots/:id` - stream a stored screenshot's bytes. A screenshot
/// is immutable once stored, so it caches aggressively. 404 when the id is unknown.
pub async fn serve(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let Some((image, mime)) = queries::get_screenshot_image(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "screenshot not found").into_response());
    };
    let content_type = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        image,
    )
        .into_response())
}
