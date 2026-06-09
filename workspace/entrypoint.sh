#!/usr/bin/env bash
# Workspace entrypoint. Runs as root to perform setup, then idles.
#
# The actual agent work runs as the non-root `node` user via `docker exec`
# (the API sets the exec user), so everything here prepares that user's world:
# a writable Claude config dir seeded from the read-only host mount, Docker
# socket access, and git wired to use the GitHub token.
set -euo pipefail

CLAUDE_CONFIG_DIR="${CLAUDE_CONFIG_DIR:-/workspace/.claude}"
HOST_CLAUDE="/host-claude"

# --- Persistent workspace owned by the agent ---------------------------------
mkdir -p /workspace
chown node:node /workspace

# --- Seed the writable Claude config dir from the read-only host mount --------
# Config (AGENTS.md, docs, manuals, skills, settings) is refreshed every boot;
# the agent's single conversation under projects/ is left untouched.
mkdir -p "${CLAUDE_CONFIG_DIR}/projects"
if [ -d "${HOST_CLAUDE}" ]; then
  for item in AGENTS.md CLAUDE.md settings.json docs manuals org skills commands; do
    if [ -e "${HOST_CLAUDE}/${item}" ]; then
      cp -r "${HOST_CLAUDE}/${item}" "${CLAUDE_CONFIG_DIR}/" 2>/dev/null || true
    fi
  done
fi
chown -R node:node "${CLAUDE_CONFIG_DIR}"

# --- Give the agent access to the host Docker socket -------------------------
if [ -S /var/run/docker.sock ]; then
  SOCK_GID="$(stat -c '%g' /var/run/docker.sock)"
  if ! getent group "${SOCK_GID}" >/dev/null 2>&1; then
    groupadd -g "${SOCK_GID}" dockerhost || true
  fi
  GROUP_NAME="$(getent group "${SOCK_GID}" | cut -d: -f1)"
  if [ -n "${GROUP_NAME}" ]; then
    usermod -aG "${GROUP_NAME}" node || true
  fi
fi

# --- Wire git to authenticate through the GitHub token (as the node user) -----
if [ -n "${GH_TOKEN:-}" ]; then
  runuser -u node -- gh auth setup-git >/dev/null 2>&1 || true
fi

# Hand off to the idle command (tail -f /dev/null).
exec "$@"
