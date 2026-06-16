-- The "compose" assistant (issue #181): a second, on-demand Claude session that
-- helps the operator draft many issues at once, kept fully separate from the main
-- agent. Its conversation and stats live in their own tables so they never touch
-- the board, the agent loop, or the main session.

-- One conversation turn of the compose assistant. Mirrors `turns`, but stands
-- alone (no task_id), so its history and statistics are isolated.
CREATE TABLE compose_turns (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    idx            INTEGER NOT NULL,
    prompt         TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'running',   -- running | completed | failed
    result_text    TEXT,
    total_cost_usd DOUBLE PRECISION,
    token_usage    JSONB,
    session_id     TEXT,
    started_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at    TIMESTAMPTZ
);

CREATE INDEX compose_turns_idx ON compose_turns (idx);

-- The append-only parsed stream-json for the compose chat (its transcript).
CREATE TABLE compose_events (
    id         BIGSERIAL PRIMARY KEY,
    turn_id    UUID NOT NULL REFERENCES compose_turns(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    type       TEXT NOT NULL,
    payload    JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX compose_events_turn_seq_idx ON compose_events (turn_id, seq);

-- A draft issue the compose assistant has scoped but not yet created. `repo_id`
-- is the optional target repo (the repo a GitHub issue is filed in, or an
-- internal ticket's repo). Cleared on reset and when the drafts are bulk-created.
CREATE TABLE issue_drafts (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title      TEXT NOT NULL,
    body       TEXT NOT NULL DEFAULT '',
    repo_id    UUID REFERENCES repositories(id) ON DELETE SET NULL,
    position   DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX issue_drafts_position_idx ON issue_drafts (position);
