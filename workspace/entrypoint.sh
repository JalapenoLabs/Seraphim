#!/usr/bin/env bash
# Workspace entrypoint. Runs as root to perform setup, then idles.
#
# The actual agent work runs as the non-root `codespace` user via `docker exec`
# (the API sets the exec user). This prepares that user's world: a writable
# Claude config dir, SSH keys for git@ cloning, and git wired to the GitHub
# token. The heavy provisioning (config repo + repo clones + setup scripts) is
# driven by the API, not here.
set -euo pipefail

AGENT_USER=codespace
AGENT_HOME="/home/${AGENT_USER}"
CLAUDE_CONFIG_DIR="${CLAUDE_CONFIG_DIR:-/workspace/.claude}"

# --- Persistent workspace owned by the agent ---------------------------------
mkdir -p /workspace "${CLAUDE_CONFIG_DIR}/projects"
chown "${AGENT_USER}:${AGENT_USER}" /workspace
chown -R "${AGENT_USER}:${AGENT_USER}" "${CLAUDE_CONFIG_DIR}" 2>/dev/null || true

# --- SSH: copy the host's keys so git@github.com clones work -----------------
if [ -d /host-ssh ]; then
  mkdir -p "${AGENT_HOME}/.ssh"
  cp -r /host-ssh/. "${AGENT_HOME}/.ssh/" 2>/dev/null || true
  # Auto-accept GitHub's host key on first connect (no interactive prompt).
  printf 'Host github.com\n  StrictHostKeyChecking accept-new\n' > "${AGENT_HOME}/.ssh/config"
  chown -R "${AGENT_USER}:${AGENT_USER}" "${AGENT_HOME}/.ssh"
  chmod 700 "${AGENT_HOME}/.ssh"
  chmod 600 "${AGENT_HOME}/.ssh/"* 2>/dev/null || true
fi

# --- Wire git to authenticate HTTPS remotes with the GitHub token ------------
if [ -n "${GH_TOKEN:-}" ]; then
  runuser -u "${AGENT_USER}" -- gh auth setup-git >/dev/null 2>&1 || true
fi

# Hand off to the idle command (tail -f /dev/null).
exec "$@"
