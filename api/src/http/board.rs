//! Board read + card movement endpoints.

use std::cmp::Ordering;
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
use crate::db::models::{
    HeartAttack, Railway, RepoSyncError, Settings, SourceKind, Task, TaskColumn, TaskStatus,
};
use crate::db::queries;
use crate::git;
use crate::orchestrator;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct BoardResponse {
    pub tasks: Vec<Task>,
    pub settings: Settings,
    /// Every railway (swimlane), `main` first then by rank, so the board can lay
    /// out one lane per railway in a single fetch and refresh them with the tasks.
    pub railways: Vec<Railway>,
    /// Unacknowledged setup-suggestion counts, keyed by task id, so a card can
    /// shout when the agent left recommendations. Tasks with none are omitted.
    pub suggestion_counts: HashMap<Uuid, i64>,
    /// Unacknowledged heart attacks (turns that died), newest first, so the board
    /// can alert the operator with the diagnostic detail until they clear them.
    pub heart_attacks: Vec<HeartAttack>,
    /// Repos whose last issue sync failed (issue #213), so the board can show a
    /// persistent banner naming each failing repo and why until it recovers.
    pub repo_sync_errors: Vec<RepoSyncError>,
}

/// `GET /api/v1/board` - every card, the org/pause settings, per-card counts of
/// open environment suggestions, and any unacknowledged heart attacks.
pub async fn get_board(State(state): State<AppState>) -> ApiResult<Json<BoardResponse>> {
    let tasks = queries::list_tasks(&state.db).await?;
    let railways = queries::list_railways(&state.db).await?;
    let mut settings = queries::get_settings(&state.db).await?;
    // Overlay the live, in-memory rate-limit cooldown (not a stored column).
    settings.cooldown_until = state.cooldown_until();
    let suggestion_counts = queries::unacknowledged_suggestion_counts(&state.db)
        .await?
        .into_iter()
        .collect();
    let heart_attacks = queries::list_unacknowledged_heart_attacks(&state.db).await?;
    let repo_sync_errors = queries::list_repo_sync_errors(&state.db).await?;
    Ok(Json(BoardResponse {
        tasks,
        settings,
        railways,
        suggestion_counts,
        heart_attacks,
        repo_sync_errors,
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

/// The key a bulk "Sort selected" reorder (issue #274) orders by. Mirrors the
/// per-column sort vocabulary on the frontend (`columnSort.ts`), minus `custom`.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BulkSort {
    IdAsc,
    IdDesc,
    CreatedAsc,
    CreatedDesc,
    UpdatedAsc,
    UpdatedDesc,
}

impl BulkSort {
    fn compare(self, a: &Task, b: &Task) -> Ordering {
        match self {
            Self::IdAsc => natural_cmp(&a.external_id, &b.external_id),
            Self::IdDesc => natural_cmp(&b.external_id, &a.external_id),
            Self::CreatedAsc => a.created_at.cmp(&b.created_at),
            Self::CreatedDesc => b.created_at.cmp(&a.created_at),
            Self::UpdatedAsc => a.updated_at.cmp(&b.updated_at),
            Self::UpdatedDesc => b.updated_at.cmp(&a.updated_at),
        }
    }
}

/// Natural (numeric-aware, case-insensitive) compare of two external ids, so `2`
/// sorts before `10` and `PROJ-2` before `PROJ-10` (matching the frontend's
/// `localeCompare(..., { numeric: true })`). Digit runs compare by value, other
/// characters lexicographically (lowercased).
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_digits(&mut ai);
                    let nb = take_digits(&mut bi);
                    // Strip leading zeros, then compare by length (magnitude) and
                    // finally lexicographically, so 002 == 2 and 10 > 9.
                    let na = na.trim_start_matches('0');
                    let nb = nb.trim_start_matches('0');
                    let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
                    if ord != Ordering::Equal {
                        return ord;
                    }
                } else {
                    let ord = ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase());
                    if ord != Ordering::Equal {
                        return ord;
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

/// Consumes and returns the leading run of ASCII digits from `chars`.
fn take_digits(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut run = String::new();
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() {
            run.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    run
}

/// Computes the new positions for a sort-selected reorder (issue #274).
///
/// Within each `(railway, column)` group the selected tasks are sorted by `sort`
/// and reassigned to the SLOTS they already occupy (their own current positions, in
/// ascending order). So the selected cards reorder among themselves while every
/// unselected card keeps its position. Returns `(id, new_position)` only for tasks
/// whose position actually changes, so a no-op sort writes nothing.
fn sorted_positions(tasks: &[Task], sort: BulkSort) -> Vec<(Uuid, f64)> {
    // Distinct (railway, column) groups, in first-seen order (avoids needing Hash
    // on TaskColumn; the group count is tiny).
    let mut keys: Vec<(Uuid, TaskColumn)> = Vec::new();
    for task in tasks {
        let key = (task.railway_id, task.board_column);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    let mut updates = Vec::new();
    for key in &keys {
        let mut group: Vec<&Task> = tasks
            .iter()
            .filter(|task| (task.railway_id, task.board_column) == *key)
            .collect();
        // The slots this group occupies, ascending (smallest = top of the column).
        let mut slots: Vec<f64> = group.iter().map(|task| task.position).collect();
        slots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
        // The tasks in their new order, then dropped into the ascending slots.
        group.sort_by(|a, b| sort.compare(a, b));
        for (task, &position) in group.iter().zip(slots.iter()) {
            // Slots are the tasks' own positions reassigned, so a card that keeps
            // its slot is bit-identical; compare bits to skip those no-op writes.
            if task.position.to_bits() != position.to_bits() {
                updates.push((task.id, position));
            }
        }
    }
    updates
}

#[derive(Debug, Deserialize)]
pub struct BulkSortRequest {
    pub ids: Vec<Uuid>,
    pub sort: BulkSort,
}

/// `POST /api/v1/tasks/bulk/sort` - reorder ONLY the selected cards by a key
/// (issue #274), within the slots they already occupy in each column. Unselected
/// cards and the cards' columns are untouched; this never re-queues a card.
pub async fn bulk_sort(
    State(state): State<AppState>,
    Json(body): Json<BulkSortRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let tasks = queries::list_tasks_by_ids(&state.db, &body.ids).await?;
    let updates = sorted_positions(&tasks, body.sort);
    for (id, position) in &updates {
        queries::set_task_position(&state.db, *id, *position).await?;
    }
    if !updates.is_empty() {
        state.notify_board();
    }
    Ok(Json(json!({ "reordered": updates.len() })))
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

    use chrono::{TimeZone, Utc};
    use sqlx::types::Json;

    fn task(
        railway: Uuid,
        external_id: &str,
        column: TaskColumn,
        position: f64,
        created: i64,
        updated: i64,
    ) -> Task {
        Task {
            id: Uuid::new_v4(),
            railway_id: railway,
            source_kind: SourceKind::Github,
            external_id: external_id.to_string(),
            repo_id: None,
            target_repo_ids: Json(Vec::new()),
            jira_board_id: None,
            title: String::new(),
            body_snapshot: String::new(),
            url: String::new(),
            author_login: None,
            author_avatar_url: None,
            external_state: None,
            board_column: column,
            position,
            status: TaskStatus::Queued,
            branch: None,
            pr_url: None,
            error: None,
            ci_fix_attempts: 0,
            review_fix_attempts: 0,
            hold: false,
            blocking: false,
            notes: String::new(),
            session_id: None,
            started_at: None,
            finished_at: None,
            last_activity_at: None,
            stats_reset_at: None,
            created_at: Utc.timestamp_opt(created, 0).single().unwrap(),
            updated_at: Utc.timestamp_opt(updated, 0).single().unwrap(),
        }
    }

    // Resolves the reorder back to the external_ids in their new top-to-bottom
    // order (ascending position), so a test can assert the resulting sequence.
    fn order_after(tasks: &[Task], sort: BulkSort) -> Vec<String> {
        let updates = sorted_positions(tasks, sort);
        // Start from each task's current position, then apply the moves.
        let mut placed: Vec<(f64, String)> = tasks
            .iter()
            .map(|task| {
                let position = updates
                    .iter()
                    .find(|(id, _)| *id == task.id)
                    .map_or(task.position, |(_, position)| *position);
                (position, task.external_id.clone())
            })
            .collect();
        placed.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        placed.into_iter().map(|(_, id)| id).collect()
    }

    #[test]
    fn natural_cmp_orders_numbers_by_value_not_lexically() {
        assert_eq!(natural_cmp("2", "10"), Ordering::Less);
        assert_eq!(natural_cmp("10", "9"), Ordering::Greater);
        assert_eq!(natural_cmp("PROJ-2", "PROJ-10"), Ordering::Less);
        assert_eq!(natural_cmp("002", "2"), Ordering::Equal);
        assert_eq!(natural_cmp("bug", "BUG"), Ordering::Equal);
        assert_eq!(natural_cmp("7", "7"), Ordering::Equal);
    }

    #[test]
    fn sort_reverses_a_descending_column_into_ascending() {
        // The issue's case: a column holding 10,9,8,7,6,5 top-to-bottom.
        let lane = Uuid::new_v4();
        let tasks: Vec<Task> = ["10", "9", "8", "7", "6", "5"]
            .iter()
            .zip([1.0, 2.0, 3.0, 4.0, 5.0, 6.0])
            .map(|(id, position)| task(lane, id, TaskColumn::Todo, position, 0, 0))
            .collect();
        assert_eq!(
            order_after(&tasks, BulkSort::IdAsc),
            ["5", "6", "7", "8", "9", "10"]
        );
        assert_eq!(
            order_after(&tasks, BulkSort::IdDesc),
            ["10", "9", "8", "7", "6", "5"]
        );
    }

    #[test]
    fn sort_reuses_only_the_selected_cards_slots() {
        // Selected cards sit at positions 1, 3, 5 (unselected ones at 2, 4 are not
        // passed in). The reorder must keep those three slots, just reordered.
        let lane = Uuid::new_v4();
        let tasks = vec![
            task(lane, "8", TaskColumn::Todo, 1.0, 0, 0),
            task(lane, "3", TaskColumn::Todo, 3.0, 0, 0),
            task(lane, "5", TaskColumn::Todo, 5.0, 0, 0),
        ];
        let updates = sorted_positions(&tasks, BulkSort::IdAsc);
        let mut slots: Vec<f64> = updates.iter().map(|(_, position)| *position).collect();
        slots.extend(
            tasks
                .iter()
                .filter(|task| !updates.iter().any(|(id, _)| id == &task.id))
                .map(|task| task.position),
        );
        slots.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(slots, vec![1.0, 3.0, 5.0]); // same three slots, nothing new
        assert_eq!(order_after(&tasks, BulkSort::IdAsc), ["3", "5", "8"]);
    }

    #[test]
    fn sort_groups_by_column_and_skips_no_op() {
        let lane = Uuid::new_v4();
        let tasks = vec![
            task(lane, "9", TaskColumn::Todo, 1.0, 0, 0),
            task(lane, "4", TaskColumn::Todo, 2.0, 0, 0),
            task(lane, "1", TaskColumn::Available, 1.0, 0, 0),
            task(lane, "2", TaskColumn::Available, 2.0, 0, 0),
        ];
        // To Do reorders (9,4 -> 4,9); Available is already ascending -> untouched.
        let updates = sorted_positions(&tasks, BulkSort::IdAsc);
        assert_eq!(updates.len(), 2);
        assert!(updates.iter().all(|(id, _)| tasks
            .iter()
            .any(|task| &task.id == id && task.board_column == TaskColumn::Todo)));
    }

    #[test]
    fn sort_by_created_time_uses_timestamps() {
        let lane = Uuid::new_v4();
        // Ids are out of created-time order, so the two keys disagree.
        let tasks = vec![
            task(lane, "1", TaskColumn::Todo, 1.0, 300, 0),
            task(lane, "2", TaskColumn::Todo, 2.0, 100, 0),
            task(lane, "3", TaskColumn::Todo, 3.0, 200, 0),
        ];
        assert_eq!(order_after(&tasks, BulkSort::CreatedAsc), ["2", "3", "1"]);
        assert_eq!(order_after(&tasks, BulkSort::CreatedDesc), ["1", "3", "2"]);
    }
}
