-- Fold issue sources into repositories: a repo can be flagged to sync its own
-- issues, and org onboarding is a one-shot bulk action (no standalone source).

ALTER TABLE repositories ADD COLUMN sync_issues BOOLEAN NOT NULL DEFAULT FALSE;

-- Only sync issues carrying all of these labels (empty = no label filter).
ALTER TABLE repositories ADD COLUMN issue_labels TEXT[] NOT NULL DEFAULT '{}';

-- The standalone issue-source entity is gone; everything is repo-driven now.
DROP TABLE IF EXISTS issue_sources;

-- Fix task identity: issue numbers are unique per repo, not globally, so the
-- same number in two repos must not collide.
ALTER TABLE tasks DROP CONSTRAINT IF EXISTS tasks_source_kind_external_id_key;
ALTER TABLE tasks
    ADD CONSTRAINT tasks_repo_external_key UNIQUE (repo_id, source_kind, external_id);
