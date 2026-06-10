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
                         │ docker exec (as user `node`, cwd /workspace)
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
  `current_session_id` (the one shared Claude session), and the secret columns
  `claude_oauth_token` / `github_token` (the API only ever exposes
  `*_token_set` booleans, never the raw values; write via `POST /settings/tokens`).
- **`repositories`** — `full_name`, `clone_url`, `default_branch`,
  `branch_template`, `setup_script` (per-repo setup), `instructions`,
  `review_policy` (NULL = inherit default), `enabled`, `sync_issues` (poll this
  repo for issues), `issue_labels` (label filter). There is **no** separate
  issue-source entity; a repo with `sync_issues` is its own source. Bulk
  onboarding is the one-shot **Import from org** action (`POST /repos/import-org`).
- **`tasks`** — the cards: `source_kind`, `external_id`, `repo_id`, `title`,
  `board_column`, `position` (fractional rank), `status`, `branch`, `pr_url`,
  `error`, `hold`, `session_id`.
- **`turns`** / **`events`** — per-task Claude invocations and the append-only
  parsed stream-json (live feed + chat history).

## How the agent runtime works (the workspace)

- Claude is **always spawned at `/workspace`** as the non-root `node` user.
- **Every enabled repo** is cloned flat at `/workspace/{repo-name}` (name = part
  after `/`), so cross-repo work is natural. The task names a focus repo + branch.
- **Instructions become files:** global → `/workspace/AGENTS.md`; per-repo →
  `/workspace/{repo}/CLAUDE.md` (Claude auto-loads them).
- **Two-tier setup:** environment setup (`settings.base_setup_script`) runs once
  per provision/recreate (install CLIs/toolchains); per-repo setup
  (`repositories.setup_script`) runs after each clone (e.g. `yarn install`).
- **`~/.claude`** comes from cloning `settings.config_repo_url` into
  `CLAUDE_CONFIG_DIR=/workspace/.claude` (git init+fetch+checkout so untracked
  `projects/` — the persisted session — survives). No host mount.
- **Claude invocation** (`api/src/claude/exec.rs`):
  `claude -p <prompt> --output-format stream-json --verbose --permission-mode bypassPermissions --model <model> [--resume <session>]`,
  exec'd as user `node`, cwd `/workspace`. All tasks resume the one shared
  `current_session_id`; Claude auto-compacts.
- **Auth:** `CLAUDE_CODE_OAUTH_TOKEN` (subscription) for Claude; mounted host
  `~/.ssh` for `git@` clones; `GH_TOKEN` for HTTPS + octocrab.

### The three orchestrator loops (`api/src/orchestrator/mod.rs`)
1. **sync** — polls every repo with `sync_issues` for open issues and upserts
   them into **Available** (never clobbers human-set column/position). Tasks are
   unique per `(repo_id, source_kind, external_id)`. Callable via `POST /sync`.
2. **agent** — single-threaded: when not paused and idle, pulls top of **To Do**,
   prepares the branch, drives one Claude turn, detects the PR, moves to
   **In Review**. One task awaited to completion before the next (no overlap).
3. **review** — for `auto_squash_merge` repos, polls CI and squash-merges when
   green → **Done**.

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

`.env` (gitignored) holds the Postgres creds (bootstrap), ports, `SSH_HOME`, and
`TS_AUTHKEY`. The **Claude OAuth + GitHub tokens are NOT in `.env`** — set them in
the Settings UI (stored in the DB; a worm scanning `.env` files can't harvest
them). The Postgres password stays in `.env` because the API needs it to connect
before it can read anything; for at-rest protection use host disk encryption
(BitLocker / LUKS), which Postgres does not do itself. The `octocrab` client is
built on demand from the DB token, and the agent's `claude`/`git` execs get the
tokens injected as env at call time. Migrations are embedded at compile time
(`sqlx::migrate!`) and run on API boot.

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
  `user: "node"`. Don't remove that.
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
- **Jira source** — not implemented. The GitHub path is folded into repos
  (`sync_issues`); Jira (issues not bound to a GitHub repo) will need its own
  modeling when added. `SourceKind` enum and `tasks.source_kind` already exist.
- **MooreslabAI human-review commenting** — `GitHubSource::comment` exists but is
  unused (`#[expect(dead_code)]`).
- **Multi-repo PRs** — one primary repo per task for PR detection; cross-repo
  edits are possible but a single task opens one PR.
- **Rate-limit handling** — surface `rate_limit` events and auto-pause (planned).
