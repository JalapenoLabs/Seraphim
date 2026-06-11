-- A single global scratchpad shown beside the kanban board. It lives on the
-- settings row but is read/written through a dedicated endpoint, not the settings
-- round-trip, so the (potentially large) text never rides along with every board
-- or settings poll. Private: stored only here, never sent anywhere.
ALTER TABLE settings ADD COLUMN notepad TEXT NOT NULL DEFAULT '';
