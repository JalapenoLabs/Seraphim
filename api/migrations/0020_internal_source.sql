-- A third ticket provider: tasks that live only in our database, with no GitHub
-- or Jira backing. Created from the "Create issue" page; the agent can comment on
-- them and open/close them through the same (now source-aware) task endpoints.
--
-- ADD VALUE must land in its own migration: Postgres won't let the new enum value
-- be *used* in the same transaction it is added in, and the next migration
-- references 'internal' in a partial index predicate.
ALTER TYPE source_kind ADD VALUE IF NOT EXISTS 'internal';
