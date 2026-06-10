-- Multi-repo workspace model + portability settings.

-- A sixth lane for issues you never want the agent to touch. Sync never
-- resurfaces a task once it sits here.
ALTER TYPE task_column ADD VALUE IF NOT EXISTS 'ignored';

-- Config repo: the git URL of your ~/.claude (e.g. git@github.com:navarrotech/agents.git).
-- The workspace clones it into CLAUDE_CONFIG_DIR so a deploy is portable across
-- machines without a host mount.
ALTER TABLE settings ADD COLUMN config_repo_url TEXT NOT NULL DEFAULT '';

-- Default branch template applied to repos auto-discovered from a GitHub org.
ALTER TABLE settings
    ADD COLUMN default_branch_template TEXT NOT NULL DEFAULT 'seraphim/issue-{number}-{slug}';
