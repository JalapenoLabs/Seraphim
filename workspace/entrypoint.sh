#!/usr/bin/env bash
# Workspace entrypoint. Runs as root to perform setup, then idles.
#
# The actual agent work runs as the non-root `node` user via `docker exec` (the
# API sets the exec user). This prepares that user's world: a writable Claude
# config dir, SSH keys for git@ cloning, and git wired to the GitHub token. The
# heavy provisioning (config repo + repo clones + setup scripts) is driven by the
# API, not here.
set -euo pipefail

CLAUDE_CONFIG_DIR="${CLAUDE_CONFIG_DIR:-/workspace/.claude}"

# --- Persistent workspace owned by the agent ---------------------------------
mkdir -p /workspace "${CLAUDE_CONFIG_DIR}/projects"
chown node:node /workspace
chown -R node:node "${CLAUDE_CONFIG_DIR}" 2>/dev/null || true

# --- SSH: copy the host's keys so git@github.com clones work -----------------
if [ -d /host-ssh ]; then
  mkdir -p /home/node/.ssh
  cp -r /host-ssh/. /home/node/.ssh/ 2>/dev/null || true
  # Auto-accept GitHub's host key on first connect (no interactive prompt).
  printf 'Host github.com\n  StrictHostKeyChecking accept-new\n' > /home/node/.ssh/config
  chown -R node:node /home/node/.ssh
  chmod 700 /home/node/.ssh
  chmod 600 /home/node/.ssh/* 2>/dev/null || true
fi

# --- Wire git to authenticate HTTPS remotes with the GitHub token ------------
if [ -n "${GH_TOKEN:-}" ]; then
  runuser -u node -- gh auth setup-git >/dev/null 2>&1 || true
fi

# Hand off to the idle command (tail -f /dev/null).
exec "$@"
