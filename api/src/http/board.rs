//! Board read + card movement endpoints.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::{Settings, Task, TaskColumn};
use crate::db::queries;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct BoardResponse {
    pub tasks: Vec<Task>,
    pub settings: Settings,
    /// Unacknowledged setup-suggestion counts, keyed by task id, so a card can
    /// shout when the agent left recommendations. Tasks with none are omitted.
    pub suggestion_counts: HashMap<Uuid, i64>,
}

/// `GET /api/v1/board` - every card, the org/pause settings, and per-card
/// counts of open environment suggestions.
pub async fn get_board(State(state): State<AppState>) -> ApiResult<Json<BoardResponse>> {
    let tasks = queries::list_tasks(&state.db).await?;
    let mut settings = queries::get_settings(&state.db).await?;
    // Overlay the live, in-memory rate-limit cooldown (not a stored column).
    settings.cooldown_until = state.cooldown_until();
    let suggestion_counts = queries::unacknowledged_suggestion_counts(&state.db)
        .await?
        .into_iter()
        .collect();
    Ok(Json(BoardResponse {
        tasks,
        settings,
        suggestion_counts,
    }))
}

#[derive(Debug, Deserialize)]
pub struct MoveRequest {
    pub column: TaskColumn,
    /// Fractional rank within the column; the client computes the midpoint
    /// between the drop neighbors.
    pub position: f64,
}

/// `POST /api/v1/tasks/:id/move` - place a card in a column at a position.
pub async fn move_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<MoveRequest>,
) -> ApiResult<Json<Task>> {
    let task = queries::move_task(&state.db, id, body.column, body.position).await?;
    state.notify_board();
    Ok(Json(task))
}

#[derive(Debug, Deserialize)]
pub struct HoldRequest {
    pub hold: bool,
}

/// `POST /api/v1/tasks/:id/hold` - flag a card so the agent skips it.
pub async fn set_hold(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<HoldRequest>,
) -> ApiResult<Json<Task>> {
    let task = queries::set_task_hold(&state.db, id, body.hold).await?;
    state.notify_board();
    Ok(Json(task))
}
