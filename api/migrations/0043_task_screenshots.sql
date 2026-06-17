-- Agent screenshots (issue #248). During a task the agent captures what it sees
-- in a real browser (via the Playwright MCP, issue #243) and uploads each image
-- with the `seraphim-screenshot` helper, so the operator can see what the agent
-- saw without rebuilding the workspace state.
--
-- The bytes live here as `bytea`, following the notification-sound precedent
-- (#58): they are NEVER returned in task/board JSON (a dedicated endpoint streams
-- them by id); list payloads carry only the metadata. The images are
-- operator-facing but still live in Postgres, so respect at-rest disk encryption
-- as with the other binary blobs and secret columns.
CREATE TABLE task_screenshots (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- Best-effort association with the turn that captured it (the latest turn at
    -- upload time). SET NULL if that turn is later pruned, so the screenshot
    -- history outlives any single turn.
    turn_id    UUID REFERENCES turns(id) ON DELETE SET NULL,
    image      BYTEA NOT NULL,
    mime       TEXT NOT NULL,
    -- Pixel dimensions when the uploader could determine them (e.g. a PNG header),
    -- so the gallery can reserve layout space; NULL when unknown.
    width      INTEGER,
    height     INTEGER,
    route      TEXT NOT NULL DEFAULT '',
    caption    TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The task view lists a task's screenshots newest first.
CREATE INDEX task_screenshots_task_idx ON task_screenshots (task_id, created_at DESC);
