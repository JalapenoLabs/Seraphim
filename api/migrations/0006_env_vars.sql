-- User-defined environment variables, injected into the agent's execs at runtime
-- (the same way the Claude/GitHub tokens are). A row flagged `is_secret` is also
-- scrubbed out of Claude's output before anything is persisted or streamed, and
-- the API only ever returns a masked preview of its value.

CREATE TABLE environment_variables (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key        TEXT NOT NULL UNIQUE,
    value      TEXT NOT NULL DEFAULT '',
    is_secret  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
