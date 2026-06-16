//! Board read + card movement endpoints.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use tracing::{info, warn};

use super::ApiResult;
use crate::db::models::{HeartAttack, Settings, SourceKind, Task, TaskColumn, TaskStatus};
use crate::db::queries;
use crate::git;
use crate::orchestrator;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct BoardResponse {
    pub tasks: Vec<Task>,
    pub settings: Settings,
    /// Unacknowledged setup-suggestion counts, keyed by task id, so a card can
    /// shout when the agent left recommendations. Tasks with none are omitted.
    pub suggestion_counts: HashMap<Uuid, i64>,
    /// Unacknowledged heart attacks (turns that died), newest first, so the board
    /// can alert the operator with the diagnostic detail until they clear them.
    pub heart_attacks: Vec<HeartAttack>,
}

/// `GET /api/v1/board` - every card, the org/pause settings, per-card counts of
/// open environment suggestions, and any unacknowledged heart attacks.
pub async fn get_board(State(state): State<AppState>) -> ApiResult<Json<BoardResponse>> {
    let tasks = queries::list_tasks(&state.db).await?;
    let mut settings = queries::get_settings(&state.db).await?;
    // Overlay the live, in-memory rate-limit cooldown (not a stored column).
    settings.cooldown_until = state.cooldown_until();
    let suggestion_counts = queries::unacknowledged_suggestion_counts(&state.db)
        .await?
        .into_iter()
        .collect();
    let heart_attacks = queries::list_unacknowledged_heart_attacks(&state.db).await?;
    Ok(Json(BoardResponse {
        tasks,
        settings,
        suggestion_counts,
        heart_attacks,
    }))
}

#[derive(Debug, Deserialize)]
pub struct MoveRequest {
    pub column: TaskColumn,
    /// Fractional rank within the column; the client computes the midpoint
    /// between the drop neighbors.
    pub position: f64,
}

/// Whether moving a card to `new_column` should stop the agent's in-flight turn.
///
/// True only when the card was the live turn (see [`orchestrator::is_active_turn`])
/// and the operator is moving it to a different lane, e.g. dragging the worked
/// card back to To Do to re-order the queue (issue #172). A reorder *within* In
/// Progress leaves the agent alone. Pure, so the rule is unit-tested below.
fn move_stops_turn(
    prev_column: TaskColumn,
    prev_status: TaskStatus,
    new_column: TaskColumn,
) -> bool {
    orchestrator::is_active_turn(prev_column, prev_status) && new_column != prev_column
}

/// `POST /api/v1/tasks/:id/move` - place a card in a column at a position.
pub async fn move_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<MoveRequest>,
) -> ApiResult<Json<Task>> {
    // Capture the card's state *before* the move: the move itself re-queues it
    // (resetting status to `queued`), which would erase the signal that it was
    // the turn the agent is actively running.
    let was_active_turn = queries::get_task(&state.db, id)
        .await?
        .is_some_and(|prev| move_stops_turn(prev.board_column, prev.status, body.column));

    let task = queries::move_task(&state.db, id, body.column, body.position).await?;

    // The operator pulled the card the agent was working out from under it (most
    // often back to To Do to re-order the queue). Stop the current turn at once so
    // the agent abandons the now-misordered work and re-picks from the board on its
    // next tick, instead of grinding the stale turn to completion first (issue #172).
    if was_active_turn {
        orchestrator::stop_active_turn(&state, &task).await?;
        info!(task = %task.id, column = ?body.column, "stopped the agent's turn: its card was moved out of In Progress");
    }

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

// --- Bulk edit ---------------------------------------------------------------
//
// Back the board's multi-select bulk actions. Each takes a set of task ids; the
// board is notified once at the end so a large selection ticks the UI a single
// time rather than per card.

#[derive(Debug, Deserialize)]
pub struct BulkFieldsRequest {
    pub ids: Vec<Uuid>,
    /// Each is `None` ("keep as is") or `Some(value)` to set across the selection.
    #[serde(default)]
    pub hold: Option<bool>,
    #[serde(default)]
    pub blocking: Option<bool>,
}

/// `POST /api/v1/tasks/bulk/fields` - set `hold` and/or `blocking` across a
/// selection of cards. Omitted fields are left untouched.
pub async fn bulk_fields(
    State(state): State<AppState>,
    Json(body): Json<BulkFieldsRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let updated = queries::bulk_set_fields(&state.db, &body.ids, body.hold, body.blocking).await?;
    state.notify_board();
    Ok(Json(json!({ "updated": updated })))
}

#[derive(Debug, Deserialize)]
pub struct BulkDeleteRequest {
    pub ids: Vec<Uuid>,
}

/// `POST /api/v1/tasks/bulk/delete` - permanently delete a selection of cards.
pub async fn bulk_delete(
    State(state): State<AppState>,
    Json(body): Json<BulkDeleteRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let deleted = queries::delete_tasks(&state.db, &body.ids).await?;
    state.notify_board();
    Ok(Json(json!({ "deleted": deleted })))
}

#[derive(Debug, Deserialize)]
pub struct BulkStatusRequest {
    pub ids: Vec<Uuid>,
    pub column: TaskColumn,
}

/// `POST /api/v1/tasks/bulk/status` - move a selection of cards into a column.
///
/// Only the operator-pickable destinations are allowed: `available`, `todo`,
/// `done`, and `ignored` (never `in_progress` / `in_review`, which the agent
/// owns). Moving to Done also closes each linked GitHub/Jira/internal ticket;
/// moving anywhere else reopens a ticket that was closed.
pub async fn bulk_status(
    State(state): State<AppState>,
    Json(body): Json<BulkStatusRequest>,
) -> ApiResult<Response> {
    if matches!(body.column, TaskColumn::InProgress | TaskColumn::InReview) {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "cannot bulk-move cards into In Progress or In Review" })),
        )
            .into_response());
    }

    // Append the selection to the bottom of the target column, preserving its
    // current relative order (the fetch is board-ordered).
    let tasks = queries::list_tasks_by_ids(&state.db, &body.ids).await?;
    let mut position = queries::max_position_in_column(&state.db, body.column)
        .await?
        .unwrap_or(0.0);

    for task in &tasks {
        position += 1.0;
        // `task` still holds the pre-move state, so check before re-queuing it.
        let interrupt = move_stops_turn(task.board_column, task.status, body.column);
        let moved = queries::move_task(&state.db, task.id, body.column, position).await?;
        // If the selection swept up the card the agent is actively working, stop
        // that turn so it pivots instead of finishing misordered work (issue #172).
        if interrupt {
            orchestrator::stop_active_turn(&state, &moved).await?;
            info!(task = %moved.id, column = ?body.column, "stopped the agent's turn: its card was bulk-moved out of In Progress");
        }
        // Reflect the move onto the source ticket (close on Done, reopen
        // otherwise). Best-effort: a service hiccup must not fail the whole move.
        if let Err(error) = sync_ticket_to_column(&state, &moved, body.column).await {
            warn!(error = %error, task = %moved.id, "failed to sync ticket state on bulk move");
        }
    }

    state.notify_board();
    Ok(Json(json!({ "updated": tasks.len() })).into_response())
}

/// Reflects a card's new column onto its source ticket: closed when it lands in
/// Done, open otherwise. A no-op when the ticket is already in the desired state.
///
/// GitHub issues are closed (reason "completed") or reopened directly; Jira
/// tickets transition through the board's column->status map (same path as a
/// drag); internal tickets just flip their stored state.
async fn sync_ticket_to_column(
    state: &AppState,
    task: &Task,
    column: TaskColumn,
) -> eyre::Result<()> {
    let desired = if column == TaskColumn::Done {
        "closed"
    } else {
        "open"
    };

    match task.source_kind {
        SourceKind::Jira => transition_jira(state, task, column).await,
        SourceKind::Internal => {
            if task.external_state.as_deref() != Some(desired) {
                queries::set_task_external_state(&state.db, task.id, desired).await?;
            }
            Ok(())
        }
        SourceKind::Github => {
            if task.external_state.as_deref() == Some(desired) {
                return Ok(());
            }
            let Some(repo_id) = task.repo_id else {
                return Ok(());
            };
            let Some(repo) = queries::get_repository(&state.db, repo_id).await? else {
                return Ok(());
            };
            let Some((owner, name)) = repo.full_name.split_once('/') else {
                return Ok(());
            };
            let github = state.github().await?;
            let reason = if desired == "closed" {
                Some("completed")
            } else {
                None
            };
            git::set_issue_state(&github, owner, name, &task.external_id, desired, reason).await?;
            queries::set_task_external_state(&state.db, task.id, desired).await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_the_worked_card_to_a_different_lane_stops_the_turn() {
        // The case from issue #172: the agent is mid-turn (In Progress + working)
        // and the operator drags the card back to To Do.
        assert!(move_stops_turn(
            TaskColumn::InProgress,
            TaskStatus::Working,
            TaskColumn::Todo,
        ));
        // Preparing counts as a live turn too, and any destination lane qualifies.
        assert!(move_stops_turn(
            TaskColumn::InProgress,
            TaskStatus::Preparing,
            TaskColumn::Available,
        ));
        assert!(move_stops_turn(
            TaskColumn::InProgress,
            TaskStatus::Working,
            TaskColumn::InReview,
        ));
    }

    #[test]
    fn reordering_within_in_progress_leaves_the_turn_running() {
        // A drop back into the same lane is a no-op reorder, not an interruption.
        assert!(!move_stops_turn(
            TaskColumn::InProgress,
            TaskStatus::Working,
            TaskColumn::InProgress,
        ));
    }

    #[test]
    fn moving_a_card_that_is_not_the_live_turn_never_stops_the_agent() {
        // An In Progress card that is only parked awaiting input is not the live
        // turn, so moving it must not kill whatever the agent is actually doing.
        assert!(!move_stops_turn(
            TaskColumn::InProgress,
            TaskStatus::WaitingForInput,
            TaskColumn::Todo,
        ));
        // A queued To Do card, or one sitting in review, is likewise never the turn.
        assert!(!move_stops_turn(
            TaskColumn::Todo,
            TaskStatus::Queued,
            TaskColumn::Available,
        ));
        assert!(!move_stops_turn(
            TaskColumn::InReview,
            TaskStatus::AwaitingReview,
            TaskColumn::Done,
        ));
    }
}
