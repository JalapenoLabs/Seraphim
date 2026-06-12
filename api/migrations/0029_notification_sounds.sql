-- Notification sounds (issue #58). The UI plays a sound when a task needs the
-- operator's attention (the agent asked a question, or a heart attack) and when a
-- task finishes (auto-merges to Done). Each event has an on/off toggle and an
-- optional custom audio clip; with no clip uploaded, the frontend falls back to a
-- bundled default chime.
--
-- The audio bytes live here but are NEVER returned in the settings payload (which
-- the board fetches constantly): a separate endpoint streams them, and the
-- settings view only exposes "is a custom clip set" booleans. This mirrors how the
-- secret token columns are handled.
ALTER TABLE settings
    ADD COLUMN attention_sound_enabled  BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN completion_sound_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN attention_sound_audio    BYTEA   NOT NULL DEFAULT ''::bytea,
    ADD COLUMN attention_sound_mime     TEXT    NOT NULL DEFAULT '',
    ADD COLUMN completion_sound_audio   BYTEA   NOT NULL DEFAULT ''::bytea,
    ADD COLUMN completion_sound_mime    TEXT    NOT NULL DEFAULT '';
