//! The global scratchpad shown beside the board. A single private value, read and
//! written on its own so the (potentially large) text stays out of the board and
//! settings payloads.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::ApiResult;
use crate::db::queries;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct NotepadResponse {
    pub content: String,
}

/// `GET /api/v1/notepad`
pub async fn get(State(state): State<AppState>) -> ApiResult<Json<NotepadResponse>> {
    let content = queries::get_notepad(&state.db).await?;
    Ok(Json(NotepadResponse { content }))
}

#[derive(Debug, Deserialize)]
pub struct NotepadRequest {
    pub content: String,
}

/// `PUT /api/v1/notepad` - save the global scratchpad. No board notification: the
/// notepad is private and changes nothing anyone else can see.
pub async fn set(
    State(state): State<AppState>,
    Json(body): Json<NotepadRequest>,
) -> ApiResult<Json<NotepadResponse>> {
    queries::set_notepad(&state.db, &body.content).await?;
    Ok(Json(NotepadResponse {
        content: body.content,
    }))
}
