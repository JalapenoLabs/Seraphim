-- Per-repo issue-sync failures, surfaced loudly instead of only logged (issue #213).
-- `sync_error` holds the last failure message (NULL when the most recent sync
-- succeeded); `sync_error_at` stamps when it was recorded. Cleared on the next
-- successful sync for the repo.
ALTER TABLE repositories
    ADD COLUMN sync_error    TEXT,
    ADD COLUMN sync_error_at TIMESTAMPTZ;
