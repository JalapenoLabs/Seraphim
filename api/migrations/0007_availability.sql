-- Availability schedule: let the agent pick up new work only during configured
-- hours/days, in the operator's own time zone, with specific dates skipped
-- (vacations, holidays). Entirely optional; disabled by default the agent runs
-- around the clock exactly as before.
--
-- Times are stored as a weekly set of windows in the operator's local time. The
-- chosen IANA time zone (e.g. 'America/Denver') is resolved at evaluation time
-- so daylight saving is handled correctly; the database itself stays in UTC.

ALTER TABLE settings ADD COLUMN availability_enabled BOOLEAN NOT NULL DEFAULT FALSE;

-- IANA time zone name the windows and skip dates are interpreted in.
ALTER TABLE settings ADD COLUMN availability_timezone TEXT NOT NULL DEFAULT 'UTC';

-- Weekly windows: [{ "weekday": 0-6 (Mon-Sun), "start_minute": 0-1440,
-- "end_minute": 0-1440 }]. An empty list means "any time of day", so a user can
-- pause for specific dates without also defining working hours.
ALTER TABLE settings ADD COLUMN availability_windows JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Calendar dates to skip entirely, as ISO strings ["2026-07-04", ...].
ALTER TABLE settings ADD COLUMN availability_skip_dates JSONB NOT NULL DEFAULT '[]'::jsonb;
