-- PR review-comment addressing before auto-merge (issue #255).
--
-- Once a task's pull requests are green and (auto-)approved, but before the final
-- merge, the review loop checks for unresolved review threads (from the org CI
-- reviewer bots AND humans). If any are present, the task is handed back to the
-- agent to address them: push commits, reply to and resolve the threads. A new
-- operational sub-state marks such a task so the agent picks it up proactively,
-- re-engages on its branch, and the review loop re-checks afterwards. It is
-- bounded by its own attempt budget; once the threads are resolved (or the budget
-- is spent) the review loop proceeds to merge, so the queue never stalls.
--
-- Adding an enum value is transaction-safe on Postgres 12+ as long as the value
-- isn't used in this same migration (it isn't).
ALTER TYPE task_status ADD VALUE IF NOT EXISTS 'addressing_review'; -- green PR with unresolved review threads; agent to address

-- How many addressing turns the agent has already spent on this task's review
-- comments. Kept separate from ci_fix_attempts so the CI-fix and review-address
-- cycles bound independently (a PR can legitimately need both).
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS review_fix_attempts INTEGER NOT NULL DEFAULT 0;
