//! Board read + card movement endpoints.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use tracing::warn;

use super::ApiResult;
use crate::db::models::{Settings, SourceKind, Task, TaskColumn};
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

    // Two-way sync: reflect the move onto a Jira ticket by transitioning its
    // workflow status. Best-effort, so a Jira hiccup never fails the board move.
    if task.source_kind == SourceKind::Jira {
        if let Err(error) = transition_jira(&state, &task, body.column).await {
            warn!(error = %error, task = %task.id, "failed to transition Jira ticket on move");
        }
    }

    Ok(Json(task))
}

/// Transitions a Jira ticket to the status its new column maps to (via the
/// board's mapping). A no-op when the task has no board, nothing maps to that
/// column, or Jira is unconfigured.
async fn transition_jira(state: &AppState, task: &Task, column: TaskColumn) -> eyre::Result<()> {
    let Some(board_id) = task.jira_board_id else {
        return Ok(());
    };
    let Some(board) = queries::get_jira_board(&state.db, board_id).await? else {
        return Ok(());
    };
    let Some(target) = crate::jira::status_for_column(&board.status_map.0, column) else {
        return Ok(());
    };
    let Some(jira) = state.jira().await? else {
        return Ok(());
    };
    if jira.transition_issue(&task.external_id, &target).await? {
        // Mirror the new status onto the card so the badge matches immediately.
        queries::set_task_external_state(&state.db, task.id, &target).await?;
        state.notify_board();
    }
    Ok(())
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

#[derive(Debug, Deserialize)]
pub struct BlockingRequest {
    pub blocking: bool,
}

/// `POST /api/v1/tasks/:id/blocking` - mark a card blocking: while it is in
/// progress, the agent starts no new work.
pub async fn set_blocking(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<BlockingRequest>,
) -> ApiResult<Json<Task>> {
    let task = queries::set_task_blocking(&state.db, id, body.blocking).await?;
    state.notify_board();
    Ok(Json(task))
}
