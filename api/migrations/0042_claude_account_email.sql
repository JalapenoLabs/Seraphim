-- The connected Claude account's email, shown next to the environment name on the
-- kanban page (issue #269).
--
-- Captured from the OAuth token-exchange (and refresh) response, which identifies
-- the authenticated account regardless of the granted scopes. Empty until a
-- subscription OAuth login (or its next token refresh) populates it; a manually
-- pasted setup-token or an API key leaves it blank, since no account identity is
-- returned on those paths.
ALTER TABLE settings
    ADD COLUMN claude_account_email TEXT NOT NULL DEFAULT '';
