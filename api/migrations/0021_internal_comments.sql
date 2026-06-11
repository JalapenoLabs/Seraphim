-- Human-friendly sequential ids for internal tickets, shown as "#42".
CREATE SEQUENCE IF NOT EXISTS internal_ticket_seq;

-- Comments on internal tickets. GitHub and Jira keep comments on their own
-- service; internal tickets need their own store. `author` is 'user' (the
-- operator, from the UI) or 'agent' (Seraphim, via the agent helper).
CREATE TABLE internal_comments (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id    UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author     TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX internal_comments_task_idx ON internal_comments (task_id, created_at);

-- An internal ticket's external_id comes from the sequence above, so it is unique
-- on its own. The composite (repo_id, source_kind, external_id) constraint treats
-- the NULL repo_id as distinct, so add a real guard against duplicates.
CREATE UNIQUE INDEX tasks_internal_external_id_key ON tasks (external_id) WHERE source_kind = 'internal';
