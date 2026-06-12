-- Dead-agent management ("heart attacks").
--
-- A turn can die mid-flight: the Claude process hangs with no output, its stream
-- breaks, or the turn aborts on an internal error. When that happens the card is
-- left stranded in In Progress and the underlying `claude -p` child can leak and
-- spin orphaned. We call that a "heart attack"; the recovery (kill the orphan,
-- revive the task, alert the operator) is the "defibrillator".
--
-- Each heart attack is recorded here so the operator is alerted in the UI and the
-- diagnostic detail survives for later patching, even after the task itself is
-- requeued, completed, or deleted.
CREATE TABLE heart_attacks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The task that died. Nullable so the incident outlives the task being deleted.
    task_id         UUID REFERENCES tasks(id) ON DELETE SET NULL,
    -- Snapshot of the task title, so the alert is readable even if the task is gone.
    task_title      TEXT NOT NULL,
    -- The task's operational status at the moment it died (e.g. 'working').
    status_label    TEXT NOT NULL DEFAULT '',
    -- The diagnosis / error logs: what we knew about why the agent died, kept so
    -- the operator can patch the underlying cause later.
    detail          TEXT NOT NULL,
    -- What the defibrillator did about it (revived, or left for a human).
    recovery        TEXT NOT NULL DEFAULT '',
    -- Cleared by the operator once they have seen it; the board banner shows the
    -- unacknowledged ones.
    acknowledged    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    acknowledged_at TIMESTAMPTZ
);

-- The board reads the unacknowledged incidents on every load; index that path.
CREATE INDEX heart_attacks_unacked_idx ON heart_attacks (created_at DESC)
    WHERE acknowledged = FALSE;
