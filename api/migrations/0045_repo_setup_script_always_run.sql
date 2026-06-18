-- Per-repo "re-run the setup script before every task" toggle (issue #275).
-- When a stacked foundation ticket merges a dependency branch that adds new
-- devDependencies, the persistent clone's node_modules go stale and the next
-- script (e.g. `cross-env ... prisma generate`) fails with "command not found".
-- With this on, the repo's `setup_script` re-runs on the existing clone before
-- each task (not just on a fresh clone), so deps are reinstalled programmatically
-- without any repo-specific logic baked into Seraphim. Defaults off so existing
-- repos keep running setup only on first clone / full provision.
ALTER TABLE repositories
    ADD COLUMN setup_script_always_run BOOLEAN NOT NULL DEFAULT FALSE;
