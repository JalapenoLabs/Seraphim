//! Task detail endpoint (card + its full event history).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{Event, Question, Task};
use crate::db::queries;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    pub task: Task,
    pub events: Vec<Event>,
    /// Every decision the agent escalated on this task, answered or pending.
    pub questions: Vec<Question>,
}

/// `GET /api/v1/tasks/:id` - the card, its conversation events, and its questions.
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
    let questions = queries::list_questions_for_task(&state.db, id).await?;
    Ok(Json(TaskDetail {
        task,
        events,
        questions,
    })
    .into_response())
}
