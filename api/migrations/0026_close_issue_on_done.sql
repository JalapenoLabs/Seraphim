-- When a GitHub-sourced task auto-merges to Done, close its linked issue with
-- state_reason "completed", so the Done column doesn't fill with issues still
-- open on GitHub. The agent merges PRs into `develop`, so GitHub's own
-- keyword-close (which fires only on the default branch) never triggers. On by
-- default; operators who prefer keyword-on-main can turn it off.
ALTER TABLE settings ADD COLUMN close_issue_on_done BOOLEAN NOT NULL DEFAULT TRUE;
