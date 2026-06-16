-- Internal tickets can target several repositories (issue #189).
--
-- `repo_id` stays the primary/focus repo the agent branches in and that gates
-- auto-pull; `target_repo_ids` is the full ordered set the operator picked. The
-- agent is told about every target for context but may open a PR in only some of
-- them. Mirrors the `jira_boards.repo_ids` shape: a JSONB array of repo UUIDs,
-- first entry == the primary `repo_id`.
ALTER TABLE tasks
    ADD COLUMN target_repo_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Backfill existing tasks that already have a single repo so their target set is
-- consistent with the new column (one entry: the current primary repo).
UPDATE tasks
SET target_repo_ids = jsonb_build_array(repo_id::text)
WHERE repo_id IS NOT NULL;
