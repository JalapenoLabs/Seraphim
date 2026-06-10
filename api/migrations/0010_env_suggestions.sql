-- Environment setup suggestions the agent makes after finishing a task: tools or
-- configuration that would make future runs on a fresh workstation go smoother
-- (e.g. "install the Rust toolchain"). They surface loudly on the board and as
-- checkboxes on the task, and stay loud until the user acknowledges them.

CREATE TABLE environment_suggestions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id         UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- A short, actionable recommendation, e.g. "Install the Rust toolchain".
    title           TEXT NOT NULL,
    -- Why it helps and how to apply it (often a setup-script snippet).
    detail          TEXT NOT NULL DEFAULT '',
    -- Checked off by the user; loud on the board until this is true.
    acknowledged    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    acknowledged_at TIMESTAMPTZ
);

CREATE INDEX environment_suggestions_task_idx ON environment_suggestions (task_id, created_at);
-- The board badge counts only the unacknowledged ones.
CREATE INDEX environment_suggestions_unack_idx
    ON environment_suggestions (task_id) WHERE acknowledged = FALSE;
