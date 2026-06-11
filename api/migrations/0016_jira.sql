-- First-class Jira support: a single Jira connection (stored on the settings row,
-- secrets in the DB like the Claude/GitHub tokens) plus the set of Jira boards we
-- follow, each with its status->column mapping and the repos its tickets target.
--
-- The agent autonomously coding a Jira ticket (which, for a board like "BUG" that
-- spans several repos, means branching and opening a PR in more than one repo) is
-- intentionally NOT wired here; that is the multi-repo execution model, still an
-- open item. This migration covers connecting, discovering boards, syncing tickets
-- in, mapping their status to our columns, and transitioning them back on a move.

-- Cloud uses email + API token (Basic auth, REST v3); Server/Data Center uses a
-- personal access token (Bearer, REST v2). The deployment picks both.
CREATE TYPE jira_deployment AS ENUM ('cloud', 'server');

ALTER TABLE settings
    ADD COLUMN jira_enabled    BOOLEAN          NOT NULL DEFAULT FALSE,
    ADD COLUMN jira_deployment jira_deployment  NOT NULL DEFAULT 'cloud',
    -- Site base URL, e.g. https://acme.atlassian.net (Cloud) or https://jira.acme.com (Server).
    ADD COLUMN jira_base_url   TEXT             NOT NULL DEFAULT '',
    -- Account email, used as the Basic-auth username on Cloud (ignored on Server).
    ADD COLUMN jira_email      TEXT             NOT NULL DEFAULT '',
    -- The API token / PAT. A secret: the API only ever exposes a masked preview.
    ADD COLUMN jira_api_token  TEXT             NOT NULL DEFAULT '';

-- The boards we follow. `board_id` is Jira's own numeric id; `status_map` maps a
-- Jira status name to one of our task_column values; `repo_ids` is the set of
-- repositories a ticket from this board may target (one ticket, possibly many
-- repos).
CREATE TABLE jira_boards (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    board_id     BIGINT  NOT NULL UNIQUE,
    name         TEXT    NOT NULL,
    project_key  TEXT    NOT NULL DEFAULT '',
    sync_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    -- { "<jira status name>": "<task_column>" }
    status_map   JSONB   NOT NULL DEFAULT '{}'::jsonb,
    -- [ "<repository uuid>", ... ]
    repo_ids     JSONB   NOT NULL DEFAULT '[]'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Which followed board a Jira task came from, so a column move can map back to a
-- Jira status and transition the ticket. NULL for GitHub tasks.
ALTER TABLE tasks
    ADD COLUMN jira_board_id UUID REFERENCES jira_boards(id) ON DELETE SET NULL;

-- A Jira issue key (e.g. "BUG-123") is unique within a site, so dedupe Jira tasks
-- on the key alone. GitHub tasks keep their existing (repo_id, source_kind,
-- external_id) uniqueness; this partial index does not touch them.
CREATE UNIQUE INDEX tasks_jira_external_id_key ON tasks (external_id) WHERE source_kind = 'jira';
