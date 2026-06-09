# Seraphim

A self-hosted **autonomous developer agent**. Seraphim runs on one machine,
watches an issue board (GitHub now, Jira later), and works tickets like a
full-blown developer would: it picks up the next issue, writes the code inside a
persistent Docker workspace, opens a pull request, and follows your repo's review
rules. You curate and order the work on a kanban board; the agent just keeps
going.

It is built around a **long-lived, single-threaded Claude Code session** so the
agent carries context from issue to issue instead of starting cold every time.
You stop being the middle-man who hand-feeds prompts.

## How it works

```
GitHub issues ──poll──▶ API ──▶ Postgres ──SSE──▶ Kanban UI
                         │                            │ (drag issue into "To Do")
                         ▼ auto-pull top of To Do     ▼
                  orchestrator (one task at a time)
                         │ docker exec
                         ▼
        workspace: claude -p --resume <session> ──stream-json──▶ live UI
                         │ (agent runs git + gh, opens a PR)
                         ▼
              API detects the PR ──▶ review policy ──▶ Done / In Review
```

The **API** (Rust) is the only orchestrator. The **workspace** is a long-lived
sandbox container with the full toolchain (Claude Code, `git`, `gh`, Docker CLI,
Earthly, Node). The API drives Claude by `docker exec`-ing into the workspace and
streaming its JSON output back to the UI.

## Services

| Service | Stack | Role |
|---|---|---|
| `postgres` | Postgres 17 | Board, issue cache, conversation log, config |
| `api` | Rust (Axum) | Issue sync, orchestrator, REST + SSE, Docker control |
| `frontend` | SvelteKit | Kanban board, live task stream, settings |
| `workspace` | Custom image | The agent sandbox (Claude Code + toolchain) |
| `tailscale` | Tailscale | Exposes the UI over your tailnet with HTTPS |

## Setup

1. **Mint a Claude subscription token** on the host (no API key needed):
   ```
   claude setup-token
   ```
   Copy the token into `CLAUDE_CODE_OAUTH_TOKEN` in your `.env`.

2. **Configure the environment**:
   ```
   cp .env.example .env
   ```
   Fill in `CLAUDE_CODE_OAUTH_TOKEN`, `GH_TOKEN`, `CLAUDE_HOME` (full path to your
   `~/.claude`), and optionally `TS_AUTHKEY` for Tailscale.

3. **Bring it up**:
   ```
   docker compose up -d --build
   ```
   (or `scripts/start.sh`)

4. Open the UI on the host (`http://localhost:3000`) or over your tailnet
   (`https://<TS_HOSTNAME>.<your-tailnet>.ts.net`).

## Using it

- Configure your repositories and a GitHub issue source in **Settings**.
- Synced issues land in the **Available** column.
- Drag the issues you want worked into **To Do** and order them top-to-bottom.
- The agent auto-pulls the top card, works it, and opens a PR (card moves to
  **In Review**). Repos set to `auto_squash_merge` are merged automatically once
  checks pass; others wait for you.
- A global **pause** in Settings stops the agent from pulling new work.

## Two deployments, one image

The same image runs everywhere; behavior differs by config:

- **Per-host `.env`** holds secrets and the `~/.claude` path.
- **The org profile** (in the UI) holds global agent instructions and the default
  review policy, so JalapenoLabs ("squash-merge if checks pass") and MooreslabAI
  (stricter, human review) behave differently.
- **Per-repo overrides** set review policy, branch template, and setup scripts.

## Hosts

Windows 11 and Linux only (all services are Linux containers; on Windows use
Docker Desktop / WSL2). macOS is not targeted.

## Notes

- Tailscale is wired into compose via `tailscale serve`. If cross-container serve
  proxying misbehaves on a given host, fall back to the published host ports and
  the `scripts/` wrappers.
- The host Docker socket is mounted into the API and workspace. If a host can't
  expose it, the agent's Docker-in-workspace features won't be available.
