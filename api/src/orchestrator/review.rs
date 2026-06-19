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

/// A green PR's review-gate state: whether it still needs the agent's attention
/// before it may merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// No unresolved review threads and no outstanding "changes requested" review.
    Clean,
    /// Unresolved review threads and/or a "changes requested" review (from reviewer
    /// bots or humans) the agent must address and resolve before the merge.
    Outstanding,
    /// The review state could not be determined this tick (a lookup error). The
    /// gate never merges on a guess, so this re-checks next tick rather than risk
    /// merging over threads we simply failed to read.
    Unknown,
}

/// A pull request as the review decision sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrReview {
    /// Still open, with its current CI verdict; `auto_merge` is its repo's policy.
    /// `review` is the PR's review-gate state, meaningful once CI is green.
    Open {
        ci: PrCi,
        auto_merge: bool,
        review: ReviewState,
    },
    Merged,
    /// Closed without merging (the agent or a human abandoned this repo's PR).
    Closed,
    /// Open but not mergeable by GitHub, so it is never merged, fixed, or
    /// re-dispatched; it just holds the task in review until a human or unblock
    /// event acts on it. Two cases: an empty net diff vs base (a parked-by-design
    /// empty draft documenting a blocker, or an empty-by-accident PR, issue #304),
    /// and a draft of any size (GitHub refuses to merge a draft until it is marked
    /// ready, issue #315). Both guard against the same merge-fails-then-re-dispatch
    /// loop.
    Parked,
}

/// What the review loop should do with the task this tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    /// At least one open PR's CI failed: hand the task back to the agent to fix.
    Fix,
    /// A green PR has unresolved review threads or a "changes requested" review:
    /// hand the task back to the agent to address and resolve them before merging.
    AddressReview,
    /// Review work is still outstanding but the addressing budget is spent: park
    /// the task for a human rather than merge over unresolved comments.
    Block,
    /// Something is still running (CI pending, a review lookup failed, or no PR
    /// detected yet); wait and re-check.
    Wait,
    /// These open, passing, auto-merge PRs (by index) should be squash-merged now.
    Merge(Vec<usize>),
    /// Every PR is settled and at least one merged: the task is Done.
    Done,
    /// Open, passing PRs remain that need a human to merge; hold in review.
    Hold,
}

/// Decides the next action for a task given its PRs. Order matters: a failing PR
/// is fixed first; then we wait on any pending CI; then the review gate, which a
/// PR must clear (zero unresolved threads, no "changes requested" review) before
/// it may merge; then, once nothing is open, finish if anything merged.
///
/// The review gate is strict: a green PR with outstanding review work is never
/// merged. While the addressing budget lasts the agent is handed it; once the
/// budget is spent the task parks for a human (`Block`) instead of merging, so a
/// genuinely unresolvable thread can't slip an unaddressed PR through. Approval is
/// not consulted: resolution state is the gate, not approval.
///
/// `review_attempts_remaining` is whether the task still has addressing turns left.
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

    // Everything open is green. The review gate comes before any merge or hold: a
    // PR with outstanding review work is addressed while the budget lasts, and
    // parked for a human once it is spent, but it is never merged over.
    if prs.iter().any(|pr| {
        matches!(
            pr,
            PrReview::Open {
                review: ReviewState::Outstanding,
                ..
            }
        )
    }) {
        return if review_attempts_remaining {
            ReviewDecision::AddressReview
        } else {
            ReviewDecision::Block
        };
    }

    // No known-outstanding review, but if any green PR's review state could not be
    // read this tick, never merge on that guess; re-check once it is known.
    if prs.iter().any(|pr| {
        matches!(
            pr,
            PrReview::Open {
                review: ReviewState::Unknown,
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

    // Nothing failing/pending/auto-mergeable. Any open PR left needs a human, and a
    // parked PR (empty, issue #304, or draft, issue #315) likewise holds the task in
    // review: it is unmergeable, so the task is not Done while it is unresolved.
    if prs
        .iter()
        .any(|pr| matches!(pr, PrReview::Open { .. } | PrReview::Parked))
    {
        return ReviewDecision::Hold;
    }

    // Nothing open or parked. Done once at least one PR merged; an all-closed task
    // just holds (no work landed) rather than being marked complete.
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
            review: ReviewState::Clean,
        }
    }

    fn open_with_comments(ci: PrCi, auto_merge: bool) -> PrReview {
        PrReview::Open {
            ci,
            auto_merge,
            review: ReviewState::Outstanding,
        }
    }

    fn open_unknown_review(ci: PrCi, auto_merge: bool) -> PrReview {
        PrReview::Open {
            ci,
            auto_merge,
            review: ReviewState::Unknown,
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
    fn exhausted_budget_parks_for_a_human_over_unresolved_comments() {
        // With no addressing turns left, an unresolved PR is NEVER merged over: it
        // parks for a human instead, for either merge policy.
        assert_eq!(
            decide(&[open_with_comments(PrCi::Passing, true)], false),
            ReviewDecision::Block
        );
        assert_eq!(
            decide(&[open_with_comments(PrCi::Passing, false)], false),
            ReviewDecision::Block
        );
    }

    #[test]
    fn an_unreadable_review_state_waits_rather_than_merging() {
        // A review lookup that failed leaves the state unknown; the gate must not
        // merge on a guess, so it waits and re-checks instead.
        assert_eq!(
            decide_ready(&[open_unknown_review(PrCi::Passing, true)]),
            ReviewDecision::Wait
        );
        // A known-outstanding PR still takes priority over an unknown one.
        assert_eq!(
            decide_ready(&[
                open_unknown_review(PrCi::Passing, true),
                open_with_comments(PrCi::Passing, true),
            ]),
            ReviewDecision::AddressReview
        );
    }

    #[test]
    fn a_parked_empty_pr_holds_and_is_never_merged_or_redispatched() {
        // A lone parked (empty draft) PR just holds in review; it is never fixed,
        // addressed, merged, or re-dispatched, no matter what CI would say.
        assert_eq!(decide_ready(&[PrReview::Parked]), ReviewDecision::Hold);
        // Budget exhausted changes nothing: a parked PR has no review work to spend
        // attempts on, so it still simply holds.
        assert_eq!(decide(&[PrReview::Parked], false), ReviewDecision::Hold);
    }

    #[test]
    fn a_parked_pr_does_not_block_merging_a_sibling_and_never_completes_the_task() {
        // The real PR in another repo still merges; the parked one is left alone.
        assert_eq!(
            decide_ready(&[PrReview::Parked, open(PrCi::Passing, true)]),
            ReviewDecision::Merge(vec![1])
        );
        // Once the sibling has merged, the task is NOT Done while a parked PR
        // remains: it holds for a human or an unblock event to act on the park.
        assert_eq!(
            decide_ready(&[PrReview::Parked, PrReview::Merged]),
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
