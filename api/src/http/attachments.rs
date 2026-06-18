//! Ticket attachments (issue #291): operator uploads on a ticket, stored and
//! served like screenshots (issue #248).
//!
//! An operator attaches an image or file to a ticket; the raw bytes are the
//! request body and live as `bytea`, NEVER returned in a task/board payload. A
//! dedicated streaming endpoint serves them by id, and the task view lists only
//! the metadata (see [`super::tasks::TaskDetail`]). Source-ticket attachments
//! (e.g. Jira) are pulled into the same table by the orchestrator at work time;
//! they reuse the same storage and the same serve endpoint.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::ApiResult;
use crate::db::queries;
use crate::state::AppState;

/// The largest attachment we accept. Generous enough for a crash log or a
/// full-page screenshot while bounding a runaway upload; the upload route raises
/// axum's default 2MB body limit to this (see `http::router`).
pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

/// Metadata the uploader passes as query params alongside the raw file body.
#[derive(Debug, Deserialize)]
pub struct CreateParams {
    /// The original file name, shown in the task view and used by the agent when it
    /// fetches the file into the workspace. Defaults to a generic name when absent.
    #[serde(default)]
    pub filename: String,
}

/// `POST /api/v1/tasks/:id/attachments?filename=..`
///
/// The raw file is the request body and its `Content-Type` becomes the stored
/// MIME. One request per file (the frontend uploads each selected file in turn),
/// which keeps this dependency-free (no multipart parser), mirroring the
/// screenshot upload. Returns the new attachment's metadata.
pub async fn create(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Query(params): Query<CreateParams>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    if queries::get_task(&state.db, task_id).await?.is_none() {
        return Ok((StatusCode::NOT_FOUND, "task not found").into_response());
    }
    if body.is_empty() {
        return Ok((StatusCode::BAD_REQUEST, "empty attachment").into_response());
    }
    if body.len() > MAX_ATTACHMENT_BYTES {
        return Ok((
            StatusCode::PAYLOAD_TOO_LARGE,
            "attachment is too large (max 25 MB)",
        )
            .into_response());
    }

    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream");
    let file_name = match params.filename.trim() {
        "" => "attachment",
        name => name,
    };

    let attachment =
        queries::create_attachment(&state.db, task_id, "operator", None, file_name, mime, &body)
            .await?;
    state.notify_board();

    Ok((StatusCode::CREATED, Json(attachment)).into_response())
}

/// `GET /api/v1/attachments/:id` - stream a stored attachment's bytes. An
/// attachment is immutable once stored, so it caches aggressively. The original
/// file name rides in `Content-Disposition` so a browser download keeps it. 404
/// when the id is unknown.
pub async fn serve(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let Some((data, mime, file_name)) = queries::get_attachment_data(&state.db, id).await? else {
        return Ok((StatusCode::NOT_FOUND, "attachment not found").into_response());
    };
    let content_type = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
    };
    // `inline` so an image previews in the browser; the sanitized file name keeps a
    // manual download readable. Quotes/control chars are stripped so the header
    // value is always well-formed regardless of the source file name.
    let safe_name: String = file_name
        .chars()
        .map(|c| if c == '"' || c.is_control() { '_' } else { c })
        .collect();
    Ok((
        [
            (header::CONTENT_TYPE, content_type),
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{safe_name}\""),
            ),
        ],
        data,
    )
        .into_response())
}
