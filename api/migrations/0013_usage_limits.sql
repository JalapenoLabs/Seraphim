-- Subscription usage-limit auto-pause. When the agent's turn reports a
-- rate-limit notice at or beyond the configured utilization threshold, the
-- orchestrator parks all new work until the limit window resets, then resumes.

-- Whether to auto-pause when approaching/hitting the subscription limit.
ALTER TABLE settings ADD COLUMN usage_limit_pause_enabled BOOLEAN NOT NULL DEFAULT TRUE;

-- Utilization percent (0-100) at which to pause. Claude reports `utilization`
-- once its window crosses the early-warning threshold (~80%), so the default
-- pauses as soon as that warning fires.
ALTER TABLE settings ADD COLUMN usage_limit_threshold INTEGER NOT NULL DEFAULT 80;

-- Runtime state: when set and still in the future, the agent is auto-paused for
-- usage; the loop clears it once the window reset time passes.
ALTER TABLE settings ADD COLUMN usage_paused_until TIMESTAMPTZ;
