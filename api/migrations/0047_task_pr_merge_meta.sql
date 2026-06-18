-- Track, per tracked pull request, whether it is a draft and whether its net diff
-- vs base is empty (zero changed files), so the review sweep can recognize a
-- parked-by-design empty draft PR (issue #304).
--
-- When the agent hits a cross-repo blocker it cannot resolve in scope, it parks
-- the ticket by opening a draft PR with a single empty commit documenting the
-- blocker. GitHub cannot squash-merge a zero-change PR, so the auto-merge fails;
-- the review sweep used to read that as a conflict and re-dispatch the ticket
-- forever. Recording these flags lets the sweep leave such a PR parked in review
-- instead of merge-attempting and re-dispatching it.
--
-- Both default FALSE, so existing rows keep the normal review/auto-merge flow
-- until the next refresh fills in their true values.
ALTER TABLE task_pull_requests
    ADD COLUMN is_draft BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN is_empty BOOLEAN NOT NULL DEFAULT FALSE;
