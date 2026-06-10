-- CI feedback loop.
--
-- After the agent opens a PR, Seraphim watches its checks. If CI fails, the task
-- is handed back to the agent to fix on the same branch, bounded by a retry cap.
-- If the agent can't (or shouldn't) fix it, the PR is left for a human.

-- New operational sub-states for a task whose PR has failing CI. Adding enum
-- values is transaction-safe on Postgres 12+ as long as they aren't used in this
-- same migration (they aren't).
ALTER TYPE task_status ADD VALUE IF NOT EXISTS 'ci_failing'; -- red CI, queued for a fix turn
ALTER TYPE task_status ADD VALUE IF NOT EXISTS 'ci_blocked'; -- agent gave up / cap hit; needs a human

-- How many fix turns the agent has already spent on this task's failing CI.
-- IF NOT EXISTS so re-applying after the migration renumber (this began life as
-- 0006 before merging in the env-vars/availability migrations) is a no-op.
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS ci_fix_attempts INTEGER NOT NULL DEFAULT 0;
