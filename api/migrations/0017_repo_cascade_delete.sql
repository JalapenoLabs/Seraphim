-- Deleting a repository should purge everything synced from it. The task ->
-- turns/events/questions/suggestions chain already cascades on delete, but
-- `tasks.repo_id` was ON DELETE SET NULL, so deleting a repo orphaned its tasks
-- on the board instead of removing them. Switch it to ON DELETE CASCADE so a
-- repo delete trickles all the way down.
--
-- (The Jira board <-> repo association is a JSONB array, not a foreign key, so it
-- cannot cascade here; `delete_repository` strips the repo id from it in the same
-- transaction.)
ALTER TABLE tasks DROP CONSTRAINT tasks_repo_id_fkey;

ALTER TABLE tasks
    ADD CONSTRAINT tasks_repo_id_fkey
    FOREIGN KEY (repo_id) REFERENCES repositories(id) ON DELETE CASCADE;
