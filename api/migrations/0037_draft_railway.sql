-- Route-planner railway integration (issue #207, part of the Railways milestone).
--
-- Lets each drafted issue (issue_drafts, from #181) carry a target railway so the
-- operator can assign a lane and order a dependency sequence in the planner, and
-- bulk-create routes the resulting board cards into that lane's To Do.
--
-- The railway is OPTIONAL on a draft. A blank choice means "main" (the default
-- lane). The "railway follows repo" invariant still wins: when a draft targets a
-- repo, the card it becomes lands on that repo's railway regardless of this
-- column; the explicit choice only takes effect for repo-less (internal) drafts.
-- ON DELETE SET NULL so deleting a railway simply drops the drafts back to the
-- default rather than cascading them away.

ALTER TABLE issue_drafts
    ADD COLUMN railway_id UUID REFERENCES railways(id) ON DELETE SET NULL;
