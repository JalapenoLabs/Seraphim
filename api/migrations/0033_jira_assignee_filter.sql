-- Restrict synced Jira tickets to those assigned to the connected account.
--
-- A single-operator deployment usually only cares about its own queue, but a
-- board can hold the whole team's tickets. With this flag on (the default), the
-- poll sync filters board issues server-side with JQL (`assignee = currentUser()`)
-- and the realtime webhook path skips issues assigned to someone else. The flag
-- can be turned off to mirror the entire board as before.

ALTER TABLE settings
    ADD COLUMN jira_assigned_to_me_only BOOLEAN NOT NULL DEFAULT TRUE,
    -- The connected account's stable identifier, captured on a successful
    -- connection test (and self-healed during sync). On Cloud this is the opaque
    -- `accountId`; on Server / Data Center it is the username (`name`). The webhook
    -- path compares an issue's assignee against it, since a webhook payload cannot
    -- run JQL. Empty until the connection has been verified once.
    ADD COLUMN jira_account_id          TEXT    NOT NULL DEFAULT '';
