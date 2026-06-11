-- A private per-task scratchpad: free-text notes only the operator sees. Stored
-- in our database and deliberately never written back to the source ticket
-- (GitHub / Jira), unlike comments or the reasoning summaries.
ALTER TABLE tasks ADD COLUMN notes TEXT NOT NULL DEFAULT '';
