-- Realtime issue sync via inbound webhooks. GitHub (and Jira) call our endpoints
-- the moment an issue is created or changed, so the board updates instantly
-- instead of waiting for the next poll. Each secret is shared with the provider
-- to authenticate the call: GitHub signs every delivery with an HMAC-SHA256 of
-- the body, and Jira presents the secret (HMAC header or URL token). Both are
-- stored in the DB like the other secrets and only ever exposed as a "set" flag.
ALTER TABLE settings
    ADD COLUMN github_webhook_secret TEXT NOT NULL DEFAULT '',
    ADD COLUMN jira_webhook_secret   TEXT NOT NULL DEFAULT '';
