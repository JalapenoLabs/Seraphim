//! Task detail endpoint (card + its full event history).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{EnvSuggestion, Event, Task};
use crate::db::queries;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    pub task: Task,
    pub events: Vec<Event>,
    /// Setup recommendations the agent made on this task.
    pub suggestions: Vec<EnvSuggestion>,
}

/// `GET /api/v1/tasks/:id` - the card, its conversation events, and its
/// environment suggestions.
pub async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::response::Response> {
    let Some(task) = queries::get_task(&state.db, id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        )
            .into_response());
    };
    let events = queries::list_events_for_task(&state.db, id).await?;
    let suggestions = queries::list_suggestions_for_task(&state.db, id).await?;
    Ok(Json(TaskDetail {
        task,
        events,
        suggestions,
    })
    .into_response())
}
