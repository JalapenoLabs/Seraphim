//! The pure decision of what the review loop should do with a task's set of pull
//! requests. A task may have several PRs (one per affected repo); the rule is
//! that every PR must pass CI and merge before the task is Done.
//!
//! Keeping this I/O-free makes the multi-repo gating logic unit-testable in
//! isolation from GitHub and the database.

/// A single PR's CI verdict while it is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrCi {
    Pending,
    Passing,
    Failing,
}

/// A pull request as the review decision sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrReview {
    /// Still open, with its current CI verdict; `auto_merge` is its repo's policy.
    Open {
        ci: PrCi,
        auto_merge: bool,
    },
    Merged,
    /// Closed without merging (the agent or a human abandoned this repo's PR).
    Closed,
}

/// What the review loop should do with the task this tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    /// At least one open PR's CI failed: hand the task back to the agent to fix.
    Fix,
    /// Something is still running (CI pending, or no PR detected yet); wait.
    Wait,
    /// These open, passing, auto-merge PRs (by index) should be squash-merged now.
    Merge(Vec<usize>),
    /// Every PR is settled and at least one merged: the task is Done.
    Done,
    /// Open, passing PRs remain that need a human to merge; hold in review.
    Hold,
}

/// Decides the next action for a task given its PRs. Order matters: a failing PR
/// is fixed first; then we wait on any pending CI; then merge what we can; then,
/// once nothing is open, finish if anything merged.
pub fn decide(prs: &[PrReview]) -> ReviewDecision {
    if prs.is_empty() {
        // Detection hasn't found a PR yet (e.g. GitHub indexing lag); re-check.
        return ReviewDecision::Wait;
    }

    if prs.iter().any(|pr| {
        matches!(
            pr,
            PrReview::Open {
                ci: PrCi::Failing,
                ..
            }
        )
    }) {
        return ReviewDecision::Fix;
    }

    if prs.iter().any(|pr| {
        matches!(
            pr,
            PrReview::Open {
                ci: PrCi::Pending,
                ..
            }
        )
    }) {
        return ReviewDecision::Wait;
    }

    let to_merge: Vec<usize> = prs
        .iter()
        .enumerate()
        .filter(|(_, pr)| {
            matches!(
                pr,
                PrReview::Open {
                    ci: PrCi::Passing,
                    auto_merge: true
                }
            )
        })
        .map(|(index, _)| index)
        .collect();
    if !to_merge.is_empty() {
        return ReviewDecision::Merge(to_merge);
    }

    // Nothing failing/pending/auto-mergeable. Any open PR left needs a human.
    if prs.iter().any(|pr| matches!(pr, PrReview::Open { .. })) {
        return ReviewDecision::Hold;
    }

    // Nothing open. Done once at least one PR merged; an all-closed task just
    // holds (no work landed) rather than being marked complete.
    if prs.iter().any(|pr| matches!(pr, PrReview::Merged)) {
        ReviewDecision::Done
    } else {
        ReviewDecision::Hold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(ci: PrCi, auto_merge: bool) -> PrReview {
        PrReview::Open { ci, auto_merge }
    }

    #[test]
    fn no_prs_waits() {
        assert_eq!(decide(&[]), ReviewDecision::Wait);
    }

    #[test]
    fn single_pr_mirrors_the_old_flow() {
        // pending -> wait, passing+auto -> merge, merged -> done.
        assert_eq!(decide(&[open(PrCi::Pending, true)]), ReviewDecision::Wait);
        assert_eq!(
            decide(&[open(PrCi::Passing, true)]),
            ReviewDecision::Merge(vec![0])
        );
        assert_eq!(decide(&[PrReview::Merged]), ReviewDecision::Done);
        assert_eq!(decide(&[open(PrCi::Failing, true)]), ReviewDecision::Fix);
    }

    #[test]
    fn any_failing_pr_triggers_a_fix() {
        let prs = [
            open(PrCi::Passing, true),
            open(PrCi::Failing, true),
            PrReview::Merged,
        ];
        assert_eq!(decide(&prs), ReviewDecision::Fix);
    }

    #[test]
    fn waits_for_a_pending_pr_before_merging_others() {
        let prs = [open(PrCi::Passing, true), open(PrCi::Pending, true)];
        assert_eq!(decide(&prs), ReviewDecision::Wait);
    }

    #[test]
    fn merges_only_the_passing_auto_merge_prs() {
        // Index 0 passing+auto, index 1 already merged, index 2 passing+auto.
        let prs = [
            open(PrCi::Passing, true),
            PrReview::Merged,
            open(PrCi::Passing, true),
        ];
        assert_eq!(decide(&prs), ReviewDecision::Merge(vec![0, 2]));
    }

    #[test]
    fn passing_human_review_pr_holds_for_a_human() {
        let prs = [PrReview::Merged, open(PrCi::Passing, false)];
        assert_eq!(decide(&prs), ReviewDecision::Hold);
    }

    #[test]
    fn done_only_when_every_pr_is_settled_and_one_merged() {
        // All merged -> done; a closed (abandoned) PR alongside merged still done.
        assert_eq!(
            decide(&[PrReview::Merged, PrReview::Merged]),
            ReviewDecision::Done
        );
        assert_eq!(
            decide(&[PrReview::Merged, PrReview::Closed]),
            ReviewDecision::Done
        );
        // All closed, nothing merged -> hold, don't falsely complete.
        assert_eq!(decide(&[PrReview::Closed]), ReviewDecision::Hold);
    }
}
