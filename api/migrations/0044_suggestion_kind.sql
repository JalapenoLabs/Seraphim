-- Follow-up work suggestions (issue #272). The agent already records environment
-- setup recommendations after a task; this adds a second KIND of suggestion: at
-- the end of a task it can also bubble up follow-up work it noticed (cleanup, tech
-- debt, dead/duplicate code, inefficient processes, security gaps, deprecations),
-- which the operator reviews and one-clicks into a tracked issue, exactly like the
-- setup suggestions.
--
-- Both kinds share this table and the whole ack / create-issue / board-badge /
-- task-view pipeline; `kind` is the only discriminator. Existing rows are
-- environment suggestions, so that is the default.
ALTER TABLE environment_suggestions
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'environment';   -- 'environment' | 'follow_up'
