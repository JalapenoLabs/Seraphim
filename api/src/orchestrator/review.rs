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
    /// `unresolved_review` is whether it still carries unresolved review threads
    /// (reviewer-bot or human) the agent should address before the merge.
    Open {
        ci: PrCi,
        auto_merge: bool,
        unresolved_review: bool,
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
    /// A green PR carries unresolved review threads: hand the task back to the
    /// agent to address the comments before merging.
    AddressReview,
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
/// is fixed first; then we wait on any pending CI; then, once everything is green,
/// unresolved review comments are addressed (while the budget lasts) before we
/// merge what we can; then, once nothing is open, finish if anything merged.
///
/// `review_attempts_remaining` is whether the task still has addressing turns left
/// in its budget. Once it is exhausted, unresolved review threads no longer block
/// the merge, so a genuinely unresolvable thread can't stall the queue forever.
pub fn decide(prs: &[PrReview], review_attempts_remaining: bool) -> ReviewDecision {
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

    // Everything open is green. Before merging (auto policy) or holding for a human
    // (human-review policy), address any unresolved review threads on a green PR,
    // for either policy, as long as the attempt budget lasts.
    if review_attempts_remaining
        && prs.iter().any(|pr| {
            matches!(
                pr,
                PrReview::Open {
                    unresolved_review: true,
                    ..
                }
            )
        })
    {
        return ReviewDecision::AddressReview;
    }

    let to_merge: Vec<usize> = prs
        .iter()
        .enumerate()
        .filter(|(_, pr)| {
            matches!(
                pr,
                PrReview::Open {
                    ci: PrCi::Passing,
                    auto_merge: true,
                    ..
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
        PrReview::Open {
            ci,
            auto_merge,
            unresolved_review: false,
        }
    }

    fn open_with_comments(ci: PrCi, auto_merge: bool) -> PrReview {
        PrReview::Open {
            ci,
            auto_merge,
            unresolved_review: true,
        }
    }

    // Most cases don't exercise the budget, so they pass it as available.
    fn decide_ready(prs: &[PrReview]) -> ReviewDecision {
        decide(prs, true)
    }

    #[test]
    fn no_prs_waits() {
        assert_eq!(decide_ready(&[]), ReviewDecision::Wait);
    }

    #[test]
    fn single_pr_mirrors_the_old_flow() {
        // pending -> wait, passing+auto -> merge, merged -> done.
        assert_eq!(
            decide_ready(&[open(PrCi::Pending, true)]),
            ReviewDecision::Wait
        );
        assert_eq!(
            decide_ready(&[open(PrCi::Passing, true)]),
            ReviewDecision::Merge(vec![0])
        );
        assert_eq!(decide_ready(&[PrReview::Merged]), ReviewDecision::Done);
        assert_eq!(
            decide_ready(&[open(PrCi::Failing, true)]),
            ReviewDecision::Fix
        );
    }

    #[test]
    fn any_failing_pr_triggers_a_fix() {
        let prs = [
            open(PrCi::Passing, true),
            open(PrCi::Failing, true),
            PrReview::Merged,
        ];
        assert_eq!(decide_ready(&prs), ReviewDecision::Fix);
    }

    #[test]
    fn waits_for_a_pending_pr_before_merging_others() {
        let prs = [open(PrCi::Passing, true), open(PrCi::Pending, true)];
        assert_eq!(decide_ready(&prs), ReviewDecision::Wait);
    }

    #[test]
    fn merges_only_the_passing_auto_merge_prs() {
        // Index 0 passing+auto, index 1 already merged, index 2 passing+auto.
        let prs = [
            open(PrCi::Passing, true),
            PrReview::Merged,
            open(PrCi::Passing, true),
        ];
        assert_eq!(decide_ready(&prs), ReviewDecision::Merge(vec![0, 2]));
    }

    #[test]
    fn passing_human_review_pr_holds_for_a_human() {
        let prs = [PrReview::Merged, open(PrCi::Passing, false)];
        assert_eq!(decide_ready(&prs), ReviewDecision::Hold);
    }

    #[test]
    fn green_pr_with_unresolved_comments_addresses_before_merge() {
        // Auto-merge policy: address the comments instead of merging.
        assert_eq!(
            decide_ready(&[open_with_comments(PrCi::Passing, true)]),
            ReviewDecision::AddressReview
        );
        // Human-review policy: still address before the human merges.
        assert_eq!(
            decide_ready(&[open_with_comments(PrCi::Passing, false)]),
            ReviewDecision::AddressReview
        );
    }

    #[test]
    fn unresolved_comments_yield_to_failing_or_pending_ci() {
        // CI takes priority: a failing/pending PR is handled before addressing.
        assert_eq!(
            decide_ready(&[open_with_comments(PrCi::Failing, true)]),
            ReviewDecision::Fix
        );
        assert_eq!(
            decide_ready(&[open_with_comments(PrCi::Pending, true)]),
            ReviewDecision::Wait
        );
    }

    #[test]
    fn exhausted_budget_merges_over_unresolved_comments() {
        // With no addressing turns left, the comments no longer block the merge,
        // so the queue can't stall on a genuinely unresolvable thread.
        assert_eq!(
            decide(&[open_with_comments(PrCi::Passing, true)], false),
            ReviewDecision::Merge(vec![0])
        );
        // And a human-review PR just holds for a human.
        assert_eq!(
            decide(&[open_with_comments(PrCi::Passing, false)], false),
            ReviewDecision::Hold
        );
    }

    #[test]
    fn done_only_when_every_pr_is_settled_and_one_merged() {
        // All merged -> done; a closed (abandoned) PR alongside merged still done.
        assert_eq!(
            decide_ready(&[PrReview::Merged, PrReview::Merged]),
            ReviewDecision::Done
        );
        assert_eq!(
            decide_ready(&[PrReview::Merged, PrReview::Closed]),
            ReviewDecision::Done
        );
        // All closed, nothing merged -> hold, don't falsely complete.
        assert_eq!(decide_ready(&[PrReview::Closed]), ReviewDecision::Hold);
    }
}
