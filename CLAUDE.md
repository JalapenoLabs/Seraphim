> **Source-of-truth policy.** The USER is the primary source of truth. This
> `CLAUDE.md` is the SECOND source of truth. The codebase is **not** a source of
> truth (it can drift). Whenever a design decision is made or changed, update
> this file in the same change. Keep it routinely up to date.
>
> **Style:** No em dashes anywhere in user-facing text (docs, UI, commits, PRs).

# Seraphim

> Project memory for agents working **on** Seraphim itself. Read this first.

## What it is

Seraphim is a **self-hosted autonomous developer agent**. It runs on one machine,
watches an issue board (GitHub now, Jira later), and works tickets like a real
developer: picks up the next issue, writes code inside a persistent Docker
workspace, opens a pull request, and follows per-repo review rules. A human
curates and orders the work on a kanban board; the agent just keeps going.

The core idea is a **long-lived, single-threaded Claude Code session**: the agent
carries context from issue to issue instead of starting cold each time, so the
human stops being the middle-man who hand-feeds prompts.

**Two planned deployments, one image, config-only differences:** a personal
laptop (JalapenoLabs, "squash-merge if checks pass") and a work laptop
(MooreslabAI, stricter human review). Behavior differs by DB settings + `.env` +
the mounted SSH keys, never by code.

## Final decisions (locked)

- **Backend:** Rust (Axum + tokio + sqlx + bollard + octocrab + eyre + mimalloc).
- **Frontend:** SvelteKit (Svelte 5 runes), SPA mode, `svelte-dnd-action` kanban.
- **DB:** Postgres 17.
- **Orchestration:** Docker Compose is the primary deployment method.
- **Exposure:** Tailscale sidecar (`tailscale serve`); `scripts/{start,stop,restart}.sh` wrappers.
- **Hosts:** Windows 11 + Linux only (all services are Linux containers). No macOS.
- **Claude auth:** subscription only, **no API key**. Token from `claude setup-token`.
- **Secrets (Claude OAuth + GitHub tokens):** stored in the **database** (Settings UI), never in `.env`; injected into the agent's execs at runtime.
- **Agent trigger:** auto-pulls the top of **To Do** when idle (global pause switch exists).
- **Workspace model:** all enabled repos cloned flat under `/workspace`; Claude spawned at `/workspace` for cross-repo work.
- **`~/.claude` provisioning:** cloned from a **config repo** into the container (no host mount).
- **Repo auth:** SSH (mounted host `~/.ssh`) with HTTPS + `GH_TOKEN` fallback.
- **Ports (non-standard, host + internal):** API **27182**, UI **31415**.

## Architecture

Five Compose services (`docker-compose.yml`):

| Service | Stack | Role |
|---|---|---|
| `postgres` | Postgres 17 | Board, issue cache, conversation log, config |
| `api` | Rust / Axum | The only orchestrator: sync, agent loop, REST + SSE, Docker control |
| `frontend` | SvelteKit | Kanban UI, live task stream, settings |
| `workspace` | Custom image | Long-lived agent sandbox (Claude Code + toolchain) |
| `tailscale` | Tailscale | Exposes the UI over the tailnet with HTTPS |

**Control flow:** the `api` is the brain. The `workspace` is a powerful-but-dumb
sandbox; the API reaches in via `docker exec` (bollard) over the mounted host
Docker socket. Issue sync and PR/merge are done deterministically from Rust
(octocrab); the agent itself uses `git`/`gh` inside the workspace.

```
GitHub issues ──poll──▶ API ──▶ Postgres ──SSE──▶ Kanban UI
                         │                            │ (drag issue into "To Do")
                         ▼ auto-pull top of To Do     ▼
                  orchestrator (one task at a time)
                         │ docker exec (as user `codespace`, cwd /workspace)
                         ▼
        workspace: claude -p --resume <session> ──stream-json──▶ live UI
                         │ (agent runs git + gh, opens a PR)
                         ▼
              API detects the PR (octocrab) ──▶ review policy ──▶ Done / In Review
```

## Repo layout

```
docker-compose.yml      .env.example        README.md      CLAUDE.md
.github/workflows/ci.yml
api/        Rust backend (see below)
frontend/   SvelteKit UI
workspace/  Dockerfile + entrypoint.sh (the agent sandbox image)
tailscale/  serve.json
scripts/    start.sh stop.sh restart.sh
```

### Backend (`api/`)
- `src/main.rs` — boot: config, DB connect+migrate, workspace handle, GitHub client, spawn loops, serve.
- `src/config.rs` — env config. `src/state.rs` — `AppState` (clone-cheap) + SSE broadcast bus.
- `src/db/` — `models.rs` (enums + `FromRow` structs), `queries.rs` (runtime sqlx), `mod.rs` (pool + migrate). Migrations in `api/migrations/`.
- `src/claude/` — `events.rs` (stream-json parser, unit-tested), `exec.rs` (the `docker exec` turn runner).
- `src/docker/` — `Workspace`: exec, restart, recreate (bollard).
- `src/tailscale/` — `Tailscale`: manages the `seraphim-tailscale` sidecar via the
  host Docker socket (exec `tailscale status/up/down/login` as root, container
  restart). Powers the Settings → Tailscale panel (`http/tailscale.rs`,
  `/api/v1/tailscale/{status,up,down,reauth,restart}`): the tailnet URL, hosting
  status, connect/disconnect, and a login URL when the node needs auth. Status
  JSON parsing is pure + unit-tested. Container name from `TAILSCALE_CONTAINER`.
- `src/sources/` — `Source` enum (GitHub; Jira is a future variant), `github.rs`, `types.rs`.
- `src/git/` — PR detection, CI-green check, squash-merge (octocrab).
- `src/orchestrator/` — `mod.rs` (the loops), `provision.rs` (workspace provisioning), `prompt.rs`.
- `src/http/` — one file per resource + `sse.rs` + `data.rs` (export/import). Routes under `/api/v1`.

### Frontend (`frontend/`)
SvelteKit, **SPA** (`src/routes/+layout.ts` sets `ssr = false`), adapter-node.
`src/lib/api.ts` (ky client, one fn per endpoint), `src/lib/types.ts`, components
in `src/lib/components/`, pages in `src/routes/`. `src/hooks.server.ts` proxies
`/api/*` to the API in production; `vite.config.ts` proxies it in dev.

## The data model (Postgres)

`board_column` (kanban lane: available / todo / in_progress / in_review / done /
**ignored**) and `status` (operational sub-state) are intentionally separate.

- **`settings`** — single row (`id=1`): org profile, `global_instructions`,
  `default_review_policy`, `agent_paused`, `claude_model`, `base_setup_script`
  (= environment setup), `config_repo_url`, `default_branch_template`,
  `current_session_id` (the one shared Claude session), the secret columns
  `claude_oauth_token` / `github_token` (the API only ever exposes
  `*_token_set` booleans plus a masked `*_token_preview`, never the raw values;
  write via `POST /settings/tokens`), and the optional **availability schedule**
  (`availability_enabled`, `availability_timezone` (IANA), `availability_windows`
  JSONB, `availability_skip_dates` JSONB). When enabled, the agent only pulls new
  work during the configured weekly windows in the operator's time zone, skipping
  listed dates; empty windows mean "any time of day". The gate is the pure,
  unit-tested `orchestrator::availability::is_available`, checked alongside
  `agent_paused`. It also holds the **notification-sound** prefs
  (`{attention,completion}_sound_enabled` toggles plus the optional uploaded
  `{attention,completion}_sound_audio`/`_mime` clips). Like the tokens, the audio
  bytes never appear in the settings payload (only `*_sound_custom` "is a clip
  set" booleans do); a dedicated `GET/POST/DELETE /settings/sounds/:kind`
  (`kind` = `attention`|`completion`) streams, uploads, or clears them. The UI
  plays the bundled default chime (`frontend/static/sounds/{attention,completion}.wav`,
  regenerable via `frontend/scripts/gen-sounds.py`) when no custom clip is set.
  Attention sounds fire on the `notification`/`heart_attack` SSE events; the
  completion sound fires on a new `task_finished` SSE event emitted from the
  review loop's auto-merge-to-Done path (`ServerEvent::TaskFinished`).
- **`environment_variables`** — user-defined `key` / `value` / `is_secret` rows,
  injected into the agent's turn and setup execs at runtime. A secret value is
  scrubbed out of Claude's output before anything is persisted or streamed
  (`secrets::Scrubber`), and the API only ever returns it masked. CRUD via
  `GET`/`PUT /settings/env`.
- **`repositories`** — `full_name`, `clone_url`, `default_branch`,
  `branch_template`, `setup_script` (per-repo setup), `instructions`,
  `review_policy` (NULL = inherit default), `enabled`, `sync_issues` (poll this
  repo for issues), `issue_labels` (label filter), and `sync_error` /
  `sync_error_at` (issue #213: the last issue-sync failure for the repo, NULL when
  the most recent sync succeeded; set when listing the repo's issues fails, cleared
  on the next clean sync). There is **no** separate issue-source entity; a repo
  with `sync_issues` is its own source. Bulk onboarding is the one-shot **Import
  from org** action (`POST /repos/import-org`).
- **`tasks`** — the cards: `source_kind`, `external_id`, `repo_id`, `title`,
  `board_column`, `position` (fractional rank), `status`, `branch`, `pr_url`,
  `error`, `hold`, `session_id`.
- **`turns`** / **`events`** — per-task Claude invocations and the append-only
  parsed stream-json (live feed + chat history). Beyond the Claude stream, the
  orchestrator injects synthetic non-Claude events into the same `events` table +
  task SSE stream so they render with no special transport: `ci` step activity
  (issue #185, `orchestrator::ci_watch`) and `lifecycle` PR/issue moments (issue
  #226, `orchestrator::emit_lifecycle_event`). A `lifecycle` payload is
  `{ action: pr_opened | pr_merged | pr_closed | issue_closed, title, url, repo,
  number }`, kept source-agnostic so a future Jira source (transition, comment)
  can reuse it; `repo` is the short repo name, set only when the task spans more
  than one repo so the feed shows a `repo#number` tag exactly when it
  disambiguates. Each fires once per genuine transition (PR detection, our
  squash-merge, an external merge/close found by the review refresh, a per-task
  reset close, and the issue-close-on-Done path). Both flow through the
  synthetic CI turn (`get_or_create_ci_turn`).
- **`environment_suggestions`** — setup recommendations the agent makes after a
  task (`title`, `detail`, `acknowledged`). Posted by the agent's
  `seraphim-suggest` helper; shown loudly on the board and as checkboxes on the
  task until the user acknowledges them. A split "Create issue" button
  (`POST /suggestions/:id/create` with `target` internal/github/jira) turns one
  into a tracked issue (internal task, a GitHub issue in the task's repo via
  octocrab, or a Jira ticket) and marks the recommendation done.
- **`questions`** — decisions the agent escalated to the user, stored on the task
  (`prompt`, up to three suggested `options`, `status`, the chosen `answer`).
  Posted by the agent's `seraphim-ask` helper, answered in the task view, and
  surfaced as toasts + native notifications + a sidebar.
- **`heart_attacks`** — recorded "heart attacks" (turns that died mid-flight),
  written by the defibrillator loop (see above), never by a request. Each holds a
  task snapshot, the status at death, the diagnostic `detail` (error logs kept for
  later patching), what the defibrillator did (`recovery`), and `acknowledged`.
  The board reads the unacknowledged ones into its payload and shows a dismissible
  red banner; `POST /heart-attacks/:id/ack` clears one.
- **`automation_rules`** — user-defined rules (Automation page). When a GitHub
  webhook delivers an issue `created`/`updated`/`comment` event, each enabled
  rule whose source + trigger match is checked against the event; if its
  condition group (AND/OR of `{field, operator, values}`, operators
  exactly/contains/has_one_of/is_empty/is_not_empty over labels/author/repo/
  title/body/comment/state) matches, its action runs (move the card to top/bottom
  of To Do). The rule shape + the pure matcher live in `src/automation/`
  (unit-tested, I/O-free); firing lives in `orchestrator::run_github_automation`
  (called from `http/webhooks.rs` and the poll sync). Source-agnostic (rules can
  target `any`), but only GitHub events are wired so far. The webhook is the
  realtime path for all triggers; the poll sync is the reliable fallback for
  `Created` rules (issue #229): when `upsert_github_issue` first inserts an issue
  (it returns whether the issue was brand-new), `sync_repo_issues` fires `Created`
  automation for it, exactly once on that first-insert transition, never re-firing
  as the same issue is re-listed each poll. So `Created` rules work without any
  webhook; `Updated` / `Comment` triggers remain webhook-only (poll-firing those
  needs change detection). When an enabled rule exists but no GitHub webhook secret
  is set, the Automation page shows a dismissible notice so a rule never sits
  silently inert.

## How the agent runtime works (the workspace)

- Claude is **always spawned at `/workspace`** as the non-root `codespace` user (the universal devcontainer image's user).
- **Every enabled repo** is cloned flat at `/workspace/{repo-name}` (name = part
  after `/`), so cross-repo work is natural. The task names a focus repo + branch.
- **Instructions become files:** global → `/workspace/AGENTS.md`; per-repo →
  `/workspace/{repo}/CLAUDE.md` (Claude auto-loads them).
- **Two-tier setup:** environment setup (`settings.base_setup_script`) runs once
  per provision/recreate (install CLIs/toolchains); per-repo setup
  (`repositories.setup_script`) runs after each clone (e.g. `yarn install`).
- **`~/.claude`** comes from cloning `settings.config_repo_url` into
  `CLAUDE_CONFIG_DIR=/workspace/.claude` (git init+fetch+checkout so untracked
  `projects/` — the persisted session — survives). No host mount. This is a
  **dedicated, hard-failing step** (`provision::provision_config_repo`): on
  failure it records `settings.config_repo_error`, the board shows a red banner,
  and `next_actionable_task` **halts the agent** (refuses to pull work) until it
  succeeds. A blank `config_repo_url` bypasses the halt (agent runs unconfigured).
- **Claude invocation** (`api/src/claude/exec.rs`):
  `claude -p <prompt> --output-format stream-json --verbose --permission-mode bypassPermissions --model <model> [--resume <session>]`,
  exec'd as user `codespace`, cwd `/workspace`. All tasks resume the one shared
  `current_session_id`; Claude auto-compacts.
- **Auth:** `CLAUDE_CODE_OAUTH_TOKEN` (subscription) for Claude; mounted host
  `~/.ssh` for `git@` clones; `GH_TOKEN` for HTTPS + octocrab.
- **Git identity (issue #214):** the entrypoint writes a SYSTEM-wide
  `git config --system user.name/email` from `GIT_USER_NAME` / `GIT_USER_EMAIL`
  (`.env`, with a safe `Seraphim` fallback), so the agent can commit in every flat
  clone under `/workspace` without per-repo setup. A repo-local `user.*` still
  overrides it.
- **Local DB validation:** PostgreSQL 17 (client + server) is baked into the
  workspace image, and `pg-ephemeral` boots a throwaway PG17 on `127.0.0.1` and
  prints a `DATABASE_URL` (`export DATABASE_URL="$(pg-ephemeral)"`), so the agent
  can verify migrations / integration tests against the same major as CI and
  prod without a daemon. The entrypoint also aligns the agent to the mounted host
  Docker socket's group, so `docker` / `earthly` work without `sudo` too.
- **Browser e2e (issue #215):** Playwright's Chromium plus its OS libraries are
  baked into the workspace image, into a shared world-readable
  `PLAYWRIGHT_BROWSERS_PATH=/ms-playwright` (the official Playwright-in-Docker
  convention) so any user finds it, so the agent can run Plunder's `yarn test:e2e`
  immediately with no per-run `playwright install --with-deps`. The browser is
  pinned to the Playwright version Plunder uses (`PLAYWRIGHT_VERSION` build arg);
  if that drifts, only the browser re-downloads on first run, never the slow apt
  dependency step.

### The orchestrator loops (`api/src/orchestrator/mod.rs`)
1. **sync** — polls every repo with `sync_issues` for open issues and upserts
   them into the **top** of **Available** (never clobbers human-set
   column/position). Tasks are unique per `(repo_id, source_kind, external_id)`.
   Callable via `POST /sync`. Realtime alternative: inbound webhooks
   (`POST /api/v1/webhooks/{github,jira}`, in `http/webhooks.rs`) apply the same
   upsert the instant an issue changes, authenticated by a per-provider shared
   secret on the settings row (GitHub HMAC-signs the body; Jira signs or carries
   `?secret=`). Both poll and webhook share `orchestrator::upsert_{github,jira}_issue`.
   Sync also **reflects external state changes** onto the board: a GitHub issue
   closed outside Seraphim moves its card to **Done**, a reopened one back to
   **Available**, and a Jira status change moves the card to its newly mapped
   column. This fires only on a genuine transition (via `queries::apply_external_
   state`, guarded by `external_state IS DISTINCT FROM`), so steady-state syncs
   never clobber human curation, and a card the agent is mid-work on
   (`in_progress`) is left alone. The poll also pulls recently-closed issues
   (`git::list_recently_closed_issues`) since the open list can't reveal a closure.
   A repo whose issue list fails no longer fails silently (issue #213): each repo
   syncs as its own fallible unit (`sync_repo_issues`), and a failure records a
   per-repo `sync_error` (with the HTTP status and, for 403/404, a "grant the token
   access" hint), shows a persistent dismissible board banner + the error on the
   repos page, and emits a one-time notification on the success->error transition
   (`ServerEvent::RepoSyncError`). The next clean sync clears it; one repo's failure
   never stops the others. (The webhook path delivers a single pre-fetched issue, so
   it has no per-repo listing step to fail.)
2. **agent** — single-threaded: when not paused, the config repo is healthy, and
   inside the availability schedule, it picks work by priority — (a) **resume** a
   task whose question the user just answered (`waiting_for_input` → deliver the
   answers via `prompt::build_resume`), (b) a PR with failing CI to fix
   (`ci_failing`), (c) a PR whose auto-merge failed on a conflict to resolve
   (`merge_conflict` → `prompt::build_merge_conflict`: merge the base in, resolve,
   keep migrations linear), (d) top of **To Do** (fresh issue → branch → Claude
   turn → detect PR → **In Review**, or **park** as `waiting_for_input` if the
   agent asked a question), then (e) when nothing else is queued, *revisit* a PR it
   gave up on (`ci_blocked`), cooldown-gated (`REVISIT_COOLDOWN`, 15 min). The agent
   asks via the `seraphim-ask` CLI and records environment recommendations via the
   `seraphim-suggest` CLI (both baked into the workspace image), posting to
   `POST /agent/questions` and `POST /agent/suggestions`; the exec injects
   `SERAPHIM_TASK_ID` + `SERAPHIM_API_URL`. One task awaited to completion before
   the next (no overlap).
3. **review** — gates each task on **all** of its pull requests. A task can span
   several repos (the agent opens a same-named branch + PR in each); every PR is
   tracked in `task_pull_requests` and the task only reaches **Done** once they
   have all merged. The pure, unit-tested `orchestrator::review::decide` takes the
   tick's action from the set of PRs (`refresh_task_prs` updates each PR's CI +
   lifecycle first): any open PR failing → hand back to the agent; any pending →
   wait; merge the green `auto_squash_merge` PRs now; once all are settled and at
   least one merged → **Done** (and, for a GitHub-sourced task, close the linked
   issue with `state_reason: "completed"` when `close_issue_on_done` is set, the
   default; best-effort); open passing human-review PRs → hold. A red PR is bounded
   by `MAX_CI_FIX_ATTEMPTS` (3) before parking `ci_blocked`; the CI-fix turn checks
   out the branch in every repo with a PR and tags each failing check with its
   `repo#pr`. A failed auto-merge (almost always a base conflict because another PR
   landed first) flags the task `merge_conflict` so the agent resolves it on its
   branch instead of giving up, bounded by the same attempt budget; if the agent
   pushes nothing (genuinely unresolvable) or the budget is exhausted, it falls
   back to `ci_blocked` for a human. The single-PR case is just a one-row set, so
   its behavior is unchanged.
4. **defibrillator** (dead-agent management) — recovers turns that die mid-flight,
   which we call a **"heart attack"**: the agent hangs with no output, its stream
   breaks, or the turn aborts internally, leaving the card stranded `in_progress`
   and the `claude -p` child possibly leaked. Detection is layered: an **in-turn
   heartbeat** in `stream_turn` (each wait for the next event is bounded by
   `HEARTBEAT_TIMEOUT`, 20 min; a longer silence ends the turn as a heart attack),
   the `agent_loop` catching a turn that aborted with an error, and a background
   **watchdog** (`defibrillator_loop`) that reaps any task left `working` with no
   activity past `WATCHDOG_TIMEOUT` (25 min, strictly above the heartbeat so it
   never races a live turn). All three funnel into `defibrillate`, which kills the
   orphaned process (`kill_agent_process`), records a `heart_attacks` incident with
   the diagnostic detail, and revives the task — requeue to **To Do** if it had no
   PR, else back to **In Review** — bounded by `MAX_DEFIBRILLATIONS` (3) before it
   leaves the task failed for a human. Each incident alerts the operator: a
   persistent, dismissible red banner on the board (carrying the error logs) plus a
   toast and native notification. The revive-vs-give-up choice is the pure,
   unit-tested `decide_recovery`.

## Ports & URLs

- API: `http://localhost:27182` (`/api/v1/...`).
- UI: `http://localhost:31415` (your bookmark).
- Chosen non-standard and below the Linux ephemeral range. There should be **no**
  `8080`/`3000` anywhere; grep before reintroducing them.

## Launch & maintain

```bash
# First time
cp .env.example .env          # fill CLAUDE_CODE_OAUTH_TOKEN, GH_TOKEN, SSH_HOME
docker compose up -d --build  # or scripts/start.sh

# Rebuild after code changes (only what changed)
docker compose up -d --build api          # backend change
docker compose up -d --build frontend     # UI change
docker compose up -d --build workspace    # workspace image change
docker compose build --no-cache <svc>     # clean rebuild if needed

# Logs / status
docker compose ps
docker compose logs api --tail 50
docker compose down           # stop, keep volumes (scripts/stop.sh)
```

`.env` (gitignored) holds the Postgres creds (bootstrap), ports, `SSH_HOME`,
`TS_AUTHKEY`, and the agent's git identity (`GIT_USER_NAME` / `GIT_USER_EMAIL`).
The **Claude OAuth + GitHub tokens are NOT in `.env`** — set them in
the Settings UI (stored in the DB; a worm scanning `.env` files can't harvest
them). The Postgres password stays in `.env` because the API needs it to connect
before it can read anything; for at-rest protection use host disk encryption
(BitLocker / LUKS), which Postgres does not do itself. The `octocrab` client is
built on demand from the DB token, and the agent's `claude`/`git` execs get the
tokens injected as env at call time. Migrations are embedded at compile time
(`sqlx::migrate!`) and run on API boot.

**Self-update** (Settings -> Updates, `src/update/`): the running image is stamped
with `GIT_SHA`/`GIT_BRANCH` (compose build args set by `scripts/start.sh` from
host git, baked in `api/Dockerfile`). The hourly check compares that to the
branch's latest commit via the GitHub API. The "Update" button refuses while a
turn is in progress, pauses the agent, then launches a detached `docker:cli`
**updater container** (via the Docker socket) that bind-mounts the host repo
(`HOST_REPO_DIR`) + socket + `SSH_HOME` and runs `git pull` + `docker compose up
-d --build`; being outside the compose project, it survives the API being rebuilt.
The UI then polls `/version` and reloads when the commit changes. `HOST_REPO_DIR`
is the only new required env for the in-app update (the check works without it).

## Local dev / checks (must pass before committing)

```bash
cd api      && cargo +1.88 fmt && cargo +1.88 clippy --all-targets && cargo +1.88 test
cd frontend && npm run check && npm run build
```

CI (`.github/workflows/ci.yml`) runs these three jobs independently (no
fail-fast) on every PR and on `main`/`develop`.

## Conventions & gotchas

- **Rust toolchain is pinned to 1.88** (`api/rust-toolchain.toml`) because some
  transitive deps ship edition2024 crates. The host default may be older — always
  use `cargo +1.88`.
- **sqlx uses runtime queries** (`query_as::<_, T>`), not the compile-time macros,
  so the crate builds without a live DB. Keep it that way.
- **Claude must run as non-root** for `bypassPermissions`; both exec sites set
  `user: "codespace"`. Don't remove that.
- **Hard reset** (`POST /api/v1/agent/reset`, `orchestrator::hard_reset`) wipes
  history + the Claude session and requeues the in-progress task for a clean-slate
  restart. It bumps an in-memory `AppState::reset_epoch`; a turn snapshots that
  epoch at its start and abandons its post-turn handling (session persist, task
  move) if it changed, so a reset landing mid-turn is never undone by the turn it
  interrupted. Keep that guard if you touch the turn-completion path.
- **Per-task hard reset** (`POST /api/v1/tasks/:id/reset`, `orchestrator::reset_task`,
  task page button) abandons one stuck task's attempt and starts it over: if the
  agent is *actively* mid-turn on it (`in_progress` + `working`/`preparing`, unique
  because the loop is single-threaded) it bumps `reset_epoch`, kills the Claude
  process (`kill_agent_process`), and clears the shared session; then best-effort
  closes the PR, deletes the branch (remote via `git::delete_remote_branch` + the
  workspace clone), reopens a closed source issue, and returns the card to
  **Available** (`queries::reset_task`, clearing branch/PR/error/session). Unlike
  the global reset it leaves other tasks, the session (when not interrupting), and
  history untouched. Returns a `ResetSummary` of what ran.
- **stream-json schema can drift** across Claude Code versions; the parser keeps
  unknown shapes as `Other` rather than failing. Verify against the installed
  version when touching `claude/events.rs`.
- **Follow the global engineering conventions** (human readability first; read the
  applicable `~/.claude/docs/*.md` before editing Rust/Docker/TS/etc.).
- **Git:** branch is `v3.0.0`; remote is `JalapenoLabs/Seraphim`. Commit + push
  only when asked. **Never** add a co-author trailer. **No em dashes** in any
  user-facing text (commits, PRs, UI).

## Status / not yet done (phase 2)

- **Real end-to-end run is unproven** — a full Claude turn → PR → auto-merge needs
  `CLAUDE_CODE_OAUTH_TOKEN` + `GH_TOKEN` set. SSH cloning of a real private repo
  (`yearloom`) is already verified.
- **Jira source** — foundation in place (`api/src/jira/`, migration `0016`):
  a dual-mode (Cloud + Server/DC) client, connection config + secret token on the
  `settings` row, followed `jira_boards` with a status->column map and a repo set,
  board auto-discovery, ticket sync into tasks, and a two-way status transition
  when a Jira card moves columns. **Not yet:** the agent auto-coding Jira tickets.
  That needs the multi-repo execution model (a "BUG" board ticket can span several
  repos), so `pick_next_todo` is GitHub-only for now; Jira tickets sync, map, and
  transition but are not auto-pulled to be worked. Posting agent comments back to
  Jira is also future.
- **MooreslabAI human-review commenting** — `GitHubSource::comment` exists but is
  unused (`#[expect(dead_code)]`).
- **Rate-limit handling** — surface `rate_limit` events and auto-pause (planned).

## Railways (planned)

> Locked design from the scoping session. Tracked by the **Railways** GitHub
> milestone. The Conductor (automated ops agent) is explicitly out of scope for now.

A **railway** is a named parallel agent lane: its own workspace container, agent
loop, Claude session, and a set of repos. A repo belongs to **exactly one** railway
for work, so a task's railway always follows its repo. An undeletable **`main`**
railway holds everything by default.

- **Board:** swimlanes. One board, each railway a horizontal lane across the
  columns. Moving a card to another railway is a repo-reassign action, not a drag.
- **Management UI:** a dedicated top-nav page (`/railways`), not a settings
  subpage. It owns lane create/rename/describe, the per-railway pause + the global
  master pause, start/stop, delete-with-confirm, the idle-stop timeout, and the
  per-repo lane assignment. The board keeps the swimlanes; the global operator
  notepad stays in settings.
- **Loops:** one **agent loop per railway** (parallel); sync, review, and
  defibrillator stay single global loops that are railway-aware.
- **Container lifecycle:** lazy start on first work; auto idle-STOP (stopped, not
  removed, so restart is fast and keeps the clones plus session).
- **Repo reassignment:** railway follows repo; blocked only while a live turn runs
  on that repo's current railway, otherwise the repo and all its tasks move.
- **Per-railway:** session, pause, name, description, repo set, lifecycle state.
  **Global:** model, setup scripts, config repo, instructions, tokens, one
  schedule, branch template, review policy, plus a new global **notepad**
  (operator scratchpad, never injected into an agent).
- **Deletion:** `main` is undeletable; deleting another railway auto-reassigns its
  repos (and their non-active tasks) to `main`, then tears down its container and
  session. Blocked while a live turn runs on it.
- **Stats:** the subscription usage gauge is global (one shared subscription) with
  an aggregate cost/tokens/time rollup; context %, cost, tokens, and time are
  per-railway, shown on each swimlane.
- **Pause:** a global master pause plus a per-railway pause; both gate work.
- **Schedule:** one global schedule for all railways.
- **Migration:** the existing setup becomes the `main` railway;
  `current_session_id` becomes main's session; all existing repos and tasks move
  to `main`; new and imported repos default to `main`.
- **Banners:** heart-attack and notification banners stay global, tagged with the
  railway they belong to.
- **Route planner (#181):** included in the milestone. The planner assigns each
  drafted issue to a railway and orders them, then bulk-create routes them to the
  right lane.
