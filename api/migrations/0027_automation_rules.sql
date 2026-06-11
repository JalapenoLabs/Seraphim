-- User-defined automation rules. When an issue is created, updated, or commented
-- on (delivered by the realtime webhooks), each enabled rule whose source and
-- trigger match is evaluated against the event; if its condition group matches,
-- its action runs (e.g. move the card to the top/bottom of To Do).
--
-- The trigger list, condition group, and action are stored as JSON so the rule
-- shape can grow without migrations; the API validates them against typed structs
-- on read and write.
CREATE TABLE automation_rules (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT    NOT NULL DEFAULT '',
    enabled     BOOLEAN NOT NULL DEFAULT TRUE,
    -- Which source the rule applies to: 'github', 'jira', 'internal', or 'any'.
    source_kind TEXT    NOT NULL DEFAULT 'github',
    -- Events that fire the rule: any of 'created', 'updated', 'comment'.
    triggers    JSONB   NOT NULL DEFAULT '[]'::jsonb,
    -- The match group: {"combinator":"and"|"or","conditions":[{field,operator,values}]}.
    criteria    JSONB   NOT NULL DEFAULT '{"combinator":"and","conditions":[]}'::jsonb,
    -- The action: {"type":"move_to_todo","position":"top"|"bottom"}.
    action      JSONB   NOT NULL DEFAULT '{"type":"move_to_todo","position":"top"}'::jsonb,
    -- Fractional rank for ordering rules in the UI and evaluation (first match wins).
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
