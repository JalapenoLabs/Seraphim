-- Questions the agent escalates to the human when it is stuck or needs a
-- decision. Each question is stored on its task so the decision can be referenced
-- later, and drives the notifications surfaced in the UI.
--
-- Flow: the agent posts one or more questions (status 'pending'); the task parks
-- in 'waiting_for_input'; the user answers in the UI; once no pending questions
-- remain, the orchestrator resumes the agent's session with the answers.

CREATE TYPE question_status AS ENUM ('pending', 'answered', 'declined');

-- How the user responded: picked a suggested option, typed something custom, or
-- declined to choose and asked to discuss it instead.
CREATE TYPE answer_kind AS ENUM ('option', 'custom', 'declined');

CREATE TABLE questions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id     UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- The question text the agent is asking.
    prompt      TEXT NOT NULL,
    -- Up to three suggested answers: [{ "title": ..., "description": ... }].
    options     JSONB NOT NULL DEFAULT '[]'::jsonb,
    status      question_status NOT NULL DEFAULT 'pending',
    -- Set once answered: how they answered and the resulting text.
    answer_kind answer_kind,
    answer      TEXT,
    -- True once the answer has been delivered to the agent in a resume turn, so a
    -- later resume does not re-deliver an already-handled answer.
    acknowledged BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    answered_at TIMESTAMPTZ
);

-- The notifications sidebar and the resume check both scan by status.
CREATE INDEX questions_task_idx ON questions (task_id, created_at);
CREATE INDEX questions_pending_idx ON questions (status) WHERE status = 'pending';

-- The task sub-state while it waits on the user.
ALTER TYPE task_status ADD VALUE IF NOT EXISTS 'waiting_for_input';
