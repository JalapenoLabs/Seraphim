-- A "blocking" task serializes the queue: while it sits in In Progress (being
-- worked or parked waiting for input), the agent pulls no new To Do work, so a
-- task that depends on it finishing first is never started in parallel.
ALTER TABLE tasks ADD COLUMN blocking BOOLEAN NOT NULL DEFAULT FALSE;
