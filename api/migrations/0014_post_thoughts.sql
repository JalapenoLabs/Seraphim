-- Optionally post a per-turn summary of the agent's reasoning back to the
-- source issue as a comment. After each turn, the agent's thoughts are condensed
-- by a separate Claude call and posted as one comment. Off by default.

ALTER TABLE settings ADD COLUMN post_thoughts_enabled BOOLEAN NOT NULL DEFAULT FALSE;
