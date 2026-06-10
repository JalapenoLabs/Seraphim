-- Store the high-value app tokens in the database instead of .env, so a host
-- worm scanning for .env files can't harvest them. Entered via the UI; never
-- returned by the API (only a "set" boolean is exposed).

ALTER TABLE settings ADD COLUMN claude_oauth_token TEXT NOT NULL DEFAULT '';
ALTER TABLE settings ADD COLUMN github_token TEXT NOT NULL DEFAULT '';
