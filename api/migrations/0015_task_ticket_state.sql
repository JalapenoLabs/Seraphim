-- The source ticket's own lifecycle state, shown on each board card next to the
-- agent's internal status. They answer different questions: `status` is where
-- *our* agent is (working, awaiting review, ...), while this is where the ticket
-- itself sits in its tracker. For GitHub that is "open" / "closed"; for Jira it
-- will be the workflow status name (e.g. "To Do" / "In Progress" / "Done").
--
-- Free-form text rather than an enum because Jira states are project-defined.
-- It is maintained wherever we learn the truth: issue sync only ever lists OPEN
-- issues, so an upsert from sync writes "open", and closing or reopening the
-- ticket from Seraphim (the control in the task view) writes the new state.
-- NULL means "not yet known".
ALTER TABLE tasks ADD COLUMN external_state TEXT;

-- Existing GitHub cards were all synced from open issues, so seed them as "open"
-- to populate the badge right away; the value self-corrects on the next sync or
-- state change. Jira rows (none yet) stay NULL.
UPDATE tasks SET external_state = 'open' WHERE source_kind = 'github';
