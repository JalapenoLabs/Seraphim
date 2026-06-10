//! Environment setup suggestions the agent makes, and the user acknowledging
//! them.
//!
//! The agent's `seraphim-suggest` helper posts recommendations
//! (`POST /agent/suggestions`); the task view checks them off
//! (`POST /suggestions/:id/ack`). They are listed as part of the task detail.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::EnvSuggestionWrite;
use crate::db::queries;
use crate::state::AppState;

/// The most suggestions a single post may record, to bound a runaway agent.
const MAX_SUGGESTIONS: usize = 10;

#[derive(Debug, Deserialize)]
pub struct SuggestRequest {
    pub task_id: Uuid,
    pub suggestions: Vec<EnvSuggestionWrite>,
}

/// `POST /api/v1/agent/suggestions` - the agent records setup recommendations.
///
/// Called from inside the workspace by `seraphim-suggest`. Blank-titled entries
/// are skipped, and the board badge lights up for the task.
pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<SuggestRequest>,
) -> ApiResult<axum::response::Response> {
    if queries::get_task(&state.db, body.task_id).await?.is_none() {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        )
            .into_response());
    }

    let mut ids = Vec::new();
    for suggestion in body.suggestions.into_iter().take(MAX_SUGGESTIONS) {
        let title = suggestion.title.trim();
        if title.is_empty() {
            continue;
        }
        let created =
            queries::create_suggestion(&state.db, body.task_id, title, suggestion.detail.trim())
                .await?;
        ids.push(created.id);
    }

    // The board badge reflects the new unacknowledged suggestions.
    state.notify_board();

    Ok(Json(json!({ "suggestion_ids": ids })).into_response())
}

#[derive(Debug, Deserialize)]
pub struct AckRequest {
    pub acknowledged: bool,
}

/// `POST /api/v1/suggestions/:id/ack` - the user checks (or unchecks) a
/// suggestion. Once acknowledged it stops being loud on the board.
pub async fn acknowledge(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AckRequest>,
) -> ApiResult<Json<crate::db::models::EnvSuggestion>> {
    let suggestion = queries::set_suggestion_acknowledged(&state.db, id, body.acknowledged).await?;
    state.notify_board();
    Ok(Json(suggestion))
}
