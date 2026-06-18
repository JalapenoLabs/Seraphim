-- Ticket attachments (issue #291): operator uploads on internal tickets, plus
-- source-ticket attachments (Jira) pulled into ticket data so the agent sees
-- them. One shared table keyed by task, mirroring `task_screenshots` (#248): the
-- bytes live here as `bytea` and are NEVER returned in board/task JSON (a
-- dedicated endpoint streams them by id); list payloads carry only metadata.
-- Operator-facing but still in Postgres, so respect at-rest disk encryption as
-- with the other binary blobs and secret columns.
CREATE TABLE task_attachments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- Where the attachment came from: an operator upload, or pulled from the
    -- source tracker. 'operator' | 'jira' | 'github'.
    source      TEXT NOT NULL DEFAULT 'operator',
    -- The source tracker's own attachment id (e.g. the Jira attachment id), used
    -- to dedupe re-pulls so a re-sync never stores the same file twice. NULL for
    -- operator uploads (each upload is its own distinct attachment).
    external_id TEXT,
    file_name   TEXT NOT NULL,
    mime        TEXT NOT NULL DEFAULT '',
    byte_size   BIGINT NOT NULL,
    data        BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The task view lists a task's attachments oldest first (upload/source order).
CREATE INDEX task_attachments_task_idx ON task_attachments (task_id, created_at);

-- Dedupe pulled source attachments: at most one row per (task, source, source id),
-- so re-syncing a Jira ticket never duplicates its attachments. Operator uploads
-- (external_id NULL) are exempt, so the same file can be uploaded more than once.
CREATE UNIQUE INDEX task_attachments_source_uniq
    ON task_attachments (task_id, source, external_id)
    WHERE external_id IS NOT NULL;
