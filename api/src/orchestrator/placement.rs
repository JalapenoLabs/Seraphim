//! Deciding where a planner-created issue's card lands the first time it syncs in.
//!
//! When the route planner bulk-creates a GitHub / Jira issue (issue #207), it
//! records a [`PendingPlacement`](crate::db::models::PendingPlacement) carrying the
//! To Do position it assigned and the lane it chose. The first sync that brings the
//! issue in consumes that placement so the card honors the planner's order (and,
//! for a repo-less issue, lane) instead of landing at the top of Available.
//!
//! The decision itself is the pure, I/O-free [`resolve`] function so it can be
//! unit-tested without a database. The overwhelmingly common case (no placement)
//! resolves to [`Placement::Default`], which the caller maps to the exact same
//! upsert it has always run, leaving ordinary issue sync byte-for-byte unchanged.

use uuid::Uuid;

use crate::db::models::{PendingPlacement, TaskColumn};

/// What the shared upsert should do with an issue, given any pending placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// No planner placement applies: place the card exactly as the default sync
    /// does (top of Available, railway following the repo). This is the common
    /// path and keeps ordinary sync unchanged.
    Default,
    /// A planner placement applies: land a brand-new card in `column` at
    /// `position`, on `railway_id` when the issue has no repo to follow.
    Placed {
        column: TaskColumn,
        position: f64,
        /// The planner's chosen lane, authoritative only for a repo-less issue; a
        /// repo-bound issue follows its repo's railway regardless of this value.
        railway_id: Option<Uuid>,
    },
}

/// Resolves the placement for an issue from an optional pending placement.
///
/// Returns [`Placement::Default`] when there is no placement (the issue was not
/// planner-created), so the caller's normal top-of-Available upsert runs untouched.
/// When a placement is present, the card is sent to **To Do** (the planner always
/// targets To Do) at the recorded position, carrying the recorded lane for the
/// repo-less case.
///
/// # Examples
/// ```ignore
/// assert_eq!(resolve(None), Placement::Default);
/// ```
pub fn resolve(pending: Option<&PendingPlacement>) -> Placement {
    match pending {
        None => Placement::Default,
        Some(placement) => Placement::Placed {
            // The planner only ever routes bulk-created cards into To Do, so the
            // landing column is fixed; the stored row carries order and lane.
            column: TaskColumn::Todo,
            position: placement.position,
            railway_id: placement.railway_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::db::models::SourceKind;

    fn placement(
        position: f64,
        railway_id: Option<Uuid>,
        repo_id: Option<Uuid>,
    ) -> PendingPlacement {
        PendingPlacement {
            id: Uuid::new_v4(),
            source_kind: SourceKind::Github,
            repo_id,
            external_id: "42".to_string(),
            position,
            railway_id,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn no_placement_resolves_to_default() {
        // The common path: an ordinary synced issue has no placement and must take
        // the unchanged top-of-Available upsert.
        assert_eq!(resolve(None), Placement::Default);
    }

    #[test]
    fn placement_lands_in_todo_at_recorded_position() {
        let resolved = resolve(Some(&placement(7.0, None, Some(Uuid::new_v4()))));
        assert_eq!(
            resolved,
            Placement::Placed {
                column: TaskColumn::Todo,
                position: 7.0,
                railway_id: None,
            }
        );
    }

    #[test]
    fn placement_carries_chosen_lane_for_repo_less_issue() {
        // A repo-less (e.g. Jira) issue carries the planner's chosen lane through to
        // the upsert, which only applies it when the issue truly has no repo.
        let lane = Uuid::new_v4();
        let resolved = resolve(Some(&placement(3.0, Some(lane), None)));
        assert_eq!(
            resolved,
            Placement::Placed {
                column: TaskColumn::Todo,
                position: 3.0,
                railway_id: Some(lane),
            }
        );
    }

    #[test]
    fn placement_position_is_preserved_exactly() {
        // Fractional ranks must pass through untouched so the planner's dependency
        // order is reproduced precisely on the board.
        let resolved = resolve(Some(&placement(123.5, None, None)));
        match resolved {
            Placement::Placed { position, .. } => assert!((position - 123.5).abs() < f64::EPSILON),
            Placement::Default => panic!("expected a placement"),
        }
    }
}
