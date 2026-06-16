-- Honor the route planner's chosen ORDER (and lane) for issues it bulk-creates on
-- an external tracker (issue #207, part of the Railways milestone).
--
-- For INTERNAL drafts the planner already lands each card directly into the chosen
-- railway's To Do in dependency order. For GitHub / Jira drafts the planner can
-- only control the order in which the external ISSUES are created; the resulting
-- board card is then placed by the sync loop, which drops every fresh issue at the
-- top of Available, so the planner's order is lost.
--
-- A `pending_placements` row remembers, keyed by the issue's identity, where its
-- card should land the first time the sync upserts it in: the To Do position the
-- planner assigned, and the chosen railway (the lane). The shared upsert consumes
-- and deletes the placement when it first creates the task, then behaves normally
-- for every later sync. Rows that are never matched (e.g. the issue is deleted on
-- the tracker before it ever syncs) are pruned by age so the table cannot grow
-- unbounded.
--
-- The "railway follows repo" invariant still wins: a repo-bound issue's card lands
-- on its repo's railway regardless of the stored `railway_id`; the placement only
-- supplies the ordering (and, for a repo-less issue, the lane). This mirrors the
-- COALESCE used by `create_internal_task_in_todo`.

CREATE TABLE pending_placements (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- The issue's identity, matching how tasks are deduped: a GitHub issue needs
    -- its repo to disambiguate its number; a Jira key is globally unique (NULL
    -- repo). `repo_id IS NOT DISTINCT FROM` matching treats NULL as a value.
    source_kind source_kind NOT NULL,
    repo_id     UUID REFERENCES repositories(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    -- The To Do position the planner assigned (its dependency order).
    position    DOUBLE PRECISION NOT NULL,
    -- The chosen lane. Only authoritative for a repo-less issue; a repo-bound
    -- issue follows its repo's railway. ON DELETE SET NULL so deleting a railway
    -- simply drops the placement back to the default lane rather than removing it.
    railway_id  UUID REFERENCES railways(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One pending placement per issue identity; a re-created draft for the same issue
-- replaces the prior intent rather than stacking duplicates.
CREATE UNIQUE INDEX pending_placements_identity_idx
    ON pending_placements (source_kind, repo_id, external_id);

-- Jira rows carry a NULL repo_id, which the composite unique index above does not
-- constrain (NULLs are distinct in a UNIQUE index), so guard the key separately.
CREATE UNIQUE INDEX pending_placements_jira_key_idx
    ON pending_placements (source_kind, external_id)
    WHERE repo_id IS NULL;
