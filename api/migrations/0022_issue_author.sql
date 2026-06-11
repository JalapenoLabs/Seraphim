-- The login and avatar of whoever opened a synced GitHub issue, captured during
-- sync so the kanban card can show their profile picture without a per-card API
-- call. NULL for tasks with no known author (Jira/internal, or older rows).
ALTER TABLE tasks ADD COLUMN author_login TEXT;
ALTER TABLE tasks ADD COLUMN author_avatar_url TEXT;
