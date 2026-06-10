-- Surface config-repo (~/.claude) setup failures. NULL = healthy (or no config
-- repo configured); a non-null message means setup failed and the agent halts.
ALTER TABLE settings ADD COLUMN config_repo_error TEXT;
