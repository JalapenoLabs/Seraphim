-- Seraphim initial schema.
--
-- The board is modeled as two orthogonal axes: `board_column` is the kanban lane
-- (stable while a card sits somewhere), and `status` is the fine-grained
-- operational sub-state that animates while a task is being worked.

-- --- Enums -------------------------------------------------------------------

CREATE TYPE source_kind AS ENUM ('github', 'jira');

CREATE TYPE review_policy AS ENUM ('auto_squash_merge', 'human_review', 'none');

CREATE TYPE task_column AS ENUM ('available', 'todo', 'in_progress', 'in_review', 'done');

CREATE TYPE task_status AS ENUM (
    'queued',
    'preparing',
    'working',
    'opening_pr',
    'awaiting_review',
    'merging',
    'done',
    'failed'
);

-- --- Settings (single-row org / environment profile) -------------------------

CREATE TABLE settings (
    id                    SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    org_name              TEXT NOT NULL DEFAULT 'Seraphim',
    global_instructions   TEXT NOT NULL DEFAULT '',
    default_review_policy review_policy NOT NULL DEFAULT 'human_review',
    agent_paused          BOOLEAN NOT NULL DEFAULT FALSE,
    claude_model          TEXT NOT NULL DEFAULT 'claude-opus-4-8[1m]',
    workspace_image_tag   TEXT NOT NULL DEFAULT 'seraphim-workspace:latest',
    base_setup_script     TEXT NOT NULL DEFAULT '',
    -- The id of the single long-lived Claude conversation all tasks resume into.
    current_session_id    TEXT,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- --- Repositories the agent may work on --------------------------------------

CREATE TABLE repositories (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    full_name       TEXT NOT NULL UNIQUE,                 -- owner/repo
    clone_url       TEXT NOT NULL,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    branch_template TEXT NOT NULL DEFAULT 'seraphim/issue-{number}-{slug}',
    setup_script    TEXT NOT NULL DEFAULT '',
    instructions    TEXT NOT NULL DEFAULT '',
    -- NULL means "fall back to settings.default_review_policy".
    review_policy   review_policy,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- --- Configured issue sources ------------------------------------------------

CREATE TABLE issue_sources (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind              source_kind NOT NULL,
    -- Provider-specific config, e.g. {"owner": "navarrotech", "labels": ["agent"]}.
    config            JSONB NOT NULL DEFAULT '{}'::jsonb,
    poll_interval_secs INTEGER NOT NULL DEFAULT 120,
    enabled           BOOLEAN NOT NULL DEFAULT TRUE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- --- Tasks (the kanban cards) ------------------------------------------------

CREATE TABLE tasks (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_kind      source_kind NOT NULL,
    external_id      TEXT NOT NULL,                       -- issue number or key
    repo_id          UUID REFERENCES repositories(id) ON DELETE SET NULL,
    title            TEXT NOT NULL,
    body_snapshot    TEXT NOT NULL DEFAULT '',
    url              TEXT NOT NULL DEFAULT '',
    board_column     task_column NOT NULL DEFAULT 'available',
    -- Fractional position within a column; midpoint insertion avoids reindexing.
    position         DOUBLE PRECISION NOT NULL DEFAULT 0,
    status           task_status NOT NULL DEFAULT 'queued',
    branch           TEXT,
    pr_url           TEXT,
    error            TEXT,
    hold             BOOLEAN NOT NULL DEFAULT FALSE,
    session_id       TEXT,
    started_at       TIMESTAMPTZ,
    finished_at      TIMESTAMPTZ,
    last_activity_at TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_kind, external_id)
);

CREATE INDEX tasks_column_position_idx ON tasks (board_column, position);

-- --- Turns (one Claude invocation against a task) ----------------------------

CREATE TABLE turns (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id        UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    idx            INTEGER NOT NULL,
    prompt         TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'running',       -- running | completed | failed
    result_text    TEXT,
    total_cost_usd DOUBLE PRECISION,
    token_usage    JSONB,
    session_id     TEXT,
    started_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at    TIMESTAMPTZ
);

CREATE INDEX turns_task_idx ON turns (task_id, idx);

-- --- Events (append-only parsed stream-json: live feed + chat history) -------

CREATE TABLE events (
    id         BIGSERIAL PRIMARY KEY,
    turn_id    UUID NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    -- assistant_text | tool_use | tool_result | system | rate_limit | result
    type       TEXT NOT NULL,
    payload    JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX events_turn_seq_idx ON events (turn_id, seq);

-- Seed the single settings row so the app always has a profile to read.
INSERT INTO settings (id) VALUES (1) ON CONFLICT (id) DO NOTHING;
