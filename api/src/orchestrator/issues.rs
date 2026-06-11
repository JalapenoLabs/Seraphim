//! Keeping the source issue's open/closed state in step with the board column.
//!
//! Moving a task into **Done** closes its issue (`completed`); moving it back out
//! of Done into **Available** or **To Do** reopens it. Every other transition is
//! a no-op, as are non-GitHub tasks and tasks without a known repo.
//!
//! Best-effort: the caller logs any GitHub hiccup and never lets it block the
//! move or the merge. GitHub's `PATCH` is idempotent, so re-closing an already
//! closed issue (or re-opening an open one) is harmless and needs no pre-check.

use eyre::Result;

use crate::db::models::{SourceKind, Task, TaskColumn};
use crate::db::queries;
use crate::git;
use crate::state::AppState;

/// Reconciles the issue's open/closed state for `task` moving from `from` to
/// `to`. Touches GitHub only for the two transitions that change the desired
/// state; returns `Ok(())` immediately for anything else.
pub async fn sync_for_move(
    state: &AppState,
    task: &Task,
    from: TaskColumn,
    to: TaskColumn,
) -> Result<()> {
    let Some((issue_state, reason)) = target_state(from, to) else {
        return Ok(());
    };

    if task.source_kind != SourceKind::Github {
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

    git::set_issue_state(
        &state.github().await?,
        owner,
        name,
        &task.external_id,
        issue_state,
        reason,
    )
    .await?;
    Ok(())
}

/// The `(state, state_reason)` an issue should have after a `from -> to` column
/// move, or `None` for transitions that shouldn't touch the ticket: entering Done
/// closes it (`completed`); leaving Done for Available/To Do reopens it.
fn target_state(from: TaskColumn, to: TaskColumn) -> Option<(&'static str, Option<&'static str>)> {
    match (from, to) {
        (from, TaskColumn::Done) if from != TaskColumn::Done => Some(("closed", Some("completed"))),
        (TaskColumn::Done, TaskColumn::Available | TaskColumn::Todo) => Some(("open", None)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_done_closes_as_completed() {
        for from in [
            TaskColumn::Available,
            TaskColumn::Todo,
            TaskColumn::InProgress,
            TaskColumn::InReview,
        ] {
            assert_eq!(
                target_state(from, TaskColumn::Done),
                Some(("closed", Some("completed"))),
                "{from:?} -> Done should close"
            );
        }
    }

    #[test]
    fn leaving_done_for_the_queue_reopens() {
        assert_eq!(
            target_state(TaskColumn::Done, TaskColumn::Available),
            Some(("open", None))
        );
        assert_eq!(
            target_state(TaskColumn::Done, TaskColumn::Todo),
            Some(("open", None))
        );
    }

    #[test]
    fn other_transitions_leave_the_issue_alone() {
        // Reordering within Done, leaving Done for a non-queue lane, and moves
        // that never involve Done all no-op.
        assert_eq!(target_state(TaskColumn::Done, TaskColumn::Done), None);
        assert_eq!(target_state(TaskColumn::Done, TaskColumn::InReview), None);
        assert_eq!(target_state(TaskColumn::Done, TaskColumn::Ignored), None);
        assert_eq!(target_state(TaskColumn::Available, TaskColumn::Todo), None);
        assert_eq!(target_state(TaskColumn::Todo, TaskColumn::InProgress), None);
    }
}
