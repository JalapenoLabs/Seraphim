-- Markers for resetting live statistics. Resetting is non-destructive: stats
-- aggregate only over turns started after the marker, so the conversation history
-- is preserved. Global stats use settings.stats_reset_at; a hard task reset
-- (re-queuing it to To Do / Available) stamps the task's own marker so its time,
-- cost, and tokens start fresh.
ALTER TABLE settings ADD COLUMN stats_reset_at TIMESTAMPTZ;
ALTER TABLE tasks ADD COLUMN stats_reset_at TIMESTAMPTZ;
