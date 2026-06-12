-- Multi-repo PR support. A task may need changes (and therefore a pull request)
-- in more than one repo; the agent branches each affected repo with the same
-- branch name and opens a PR in each. This table tracks every PR a task has, so
-- the review loop can gate on ALL of them: all CI must pass and all must merge
-- before the task moves to Done. The single-PR case is just a row count of one.
--
-- `task.pr_url` is kept as the primary (first) PR for the board card; the full
-- set lives here.
CREATE TABLE task_pull_requests (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id        UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    -- The repo the PR is in. Nullable so a PR survives the repo being deleted.
    repo_id        UUID REFERENCES repositories(id) ON DELETE SET NULL,
    repo_full_name TEXT   NOT NULL,
    pr_number      BIGINT NOT NULL,
    pr_url         TEXT   NOT NULL,
    head_sha       TEXT   NOT NULL DEFAULT '',
    -- The open PR's CI verdict: 'pending' | 'passing' | 'failing'. Meaningless
    -- once the PR is no longer open.
    ci_state       TEXT   NOT NULL DEFAULT 'pending',
    -- The PR lifecycle: 'open' | 'merged' | 'closed'.
    pr_state       TEXT   NOT NULL DEFAULT 'open',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (task_id, repo_full_name, pr_number)
);

CREATE INDEX task_pull_requests_task_idx ON task_pull_requests (task_id);
