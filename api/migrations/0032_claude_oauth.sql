-- Claude subscription OAuth login.
--
-- The long-lived inference token the agent runs on continues to live in
-- `claude_oauth_token` (injected as CLAUDE_CODE_OAUTH_TOKEN, or ANTHROPIC_API_KEY
-- in api_key mode). These columns add the auth mode and the short-lived,
-- refreshing OAuth credentials used ONLY to poll /api/oauth/usage for the
-- subscription usage gauge. They are kept separate so the agent's inference never
-- depends on the refresh loop.
--
-- `claude_auth_mode` defaults to 'subscription' so existing installs (a manual
-- setup-token in claude_oauth_token) keep using CLAUDE_CODE_OAUTH_TOKEN unchanged.

CREATE TYPE claude_auth_mode AS ENUM ('subscription', 'api_key');

ALTER TABLE settings
    ADD COLUMN claude_auth_mode claude_auth_mode NOT NULL DEFAULT 'subscription',
    ADD COLUMN claude_usage_access_token TEXT NOT NULL DEFAULT '',
    ADD COLUMN claude_usage_refresh_token TEXT NOT NULL DEFAULT '',
    ADD COLUMN claude_usage_expires_at TIMESTAMPTZ,
    ADD COLUMN claude_usage_scopes TEXT NOT NULL DEFAULT '';
