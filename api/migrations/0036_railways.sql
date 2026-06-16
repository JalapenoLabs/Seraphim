-- Railways: parallel agent lanes (issue #201, foundation of the Railways milestone).
--
-- A railway is a named agent lane with its own workspace container, agent loop,
-- Claude session, and a set of repos. A repo belongs to exactly one railway for
-- work, so a task's railway always follows its repo. An undeletable `main`
-- railway holds everything by default.
--
-- This migration is the DATA LAYER ONLY: it introduces the table, wires repos and
-- tasks to a railway, and folds the existing single-agent setup into `main`.
-- Behavior is unchanged after it lands; the orchestrator still reads
-- `settings.current_session_id` (which now mirrors main's session) exactly as
-- before. Session ownership is not rewired here (that is a later issue).

-- --- Enums -------------------------------------------------------------------

-- The lifecycle of a railway's workspace container. Containers start lazily on
-- first work and idle-STOP (stopped, not removed) so a restart is fast and keeps
-- the clones plus session. `starting` / `stopping` are the in-flight transitions.
CREATE TYPE railway_state AS ENUM ('stopped', 'starting', 'running', 'stopping');

-- --- Railways ----------------------------------------------------------------

CREATE TABLE railways (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    -- The id of this railway's long-lived Claude conversation, '' until first run.
    session_id      TEXT NOT NULL DEFAULT '',
    -- Per-railway pause; gates work alongside the global master pause.
    paused          BOOLEAN NOT NULL DEFAULT FALSE,
    lifecycle_state railway_state NOT NULL DEFAULT 'stopped',
    -- Exactly one row is the undeletable `main` railway (enforced below).
    is_main         BOOLEAN NOT NULL DEFAULT FALSE,
    -- Fractional rank for swimlane ordering; midpoint insertion avoids reindexing.
    position        DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- At most one `main` railway. A partial unique index over the constant TRUE makes
-- a second `is_main = TRUE` row a hard error while leaving the FALSE rows free.
CREATE UNIQUE INDEX railways_single_main_idx ON railways (is_main) WHERE is_main;

CREATE INDEX railways_position_idx ON railways (position);

-- --- Create the `main` railway and fold the existing setup into it ------------

-- `main` owns everything by default. Its session is the current shared session,
-- so the orchestrator (which still reads settings.current_session_id) is unchanged.
INSERT INTO railways (name, is_main, session_id)
SELECT 'main', TRUE, COALESCE(current_session_id, '')
FROM settings
WHERE id = 1;

-- --- Wire repositories and tasks to a railway --------------------------------

-- Added nullable first so existing rows can be backfilled, then made NOT NULL.
-- ON DELETE RESTRICT: a railway's repos must be reassigned before it is deleted
-- (deleting a railway hands its repos back to `main`); `main` is undeletable.
ALTER TABLE repositories
    ADD COLUMN railway_id UUID REFERENCES railways(id) ON DELETE RESTRICT;
ALTER TABLE tasks
    ADD COLUMN railway_id UUID REFERENCES railways(id) ON DELETE RESTRICT;

-- Backfill every existing repo and task onto `main`.
UPDATE repositories SET railway_id = (SELECT id FROM railways WHERE is_main);
UPDATE tasks SET railway_id = (SELECT id FROM railways WHERE is_main);

ALTER TABLE repositories ALTER COLUMN railway_id SET NOT NULL;
ALTER TABLE tasks ALTER COLUMN railway_id SET NOT NULL;

CREATE INDEX repositories_railway_idx ON repositories (railway_id);
CREATE INDEX tasks_railway_idx ON tasks (railway_id);
