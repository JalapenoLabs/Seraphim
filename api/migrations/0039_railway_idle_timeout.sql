-- The idle-stop reaper (orchestrator::railway::reaper) STOPS a non-main railway's
-- workspace container after it has had no work for this many minutes, freeing its
-- memory while keeping the clones + session for a fast restart. It was a hardcoded
-- 30-minute constant; promote it to an operator-tunable setting so a deployment
-- can keep lanes warm longer (or shorter) without a code change. A value of 0 (or
-- less) means "never idle-stop": the reaper leaves every lane running until it is
-- stopped by hand. The default preserves the historical 30-minute behavior.
ALTER TABLE settings
    ADD COLUMN railway_idle_timeout_minutes INTEGER NOT NULL DEFAULT 30;
