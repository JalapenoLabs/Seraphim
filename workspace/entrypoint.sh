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

# --- Docker: let the non-root agent use the mounted host socket --------------
# docker-compose bind-mounts the host's /var/run/docker.sock so the agent can run
# docker (and Earthly) for the repos it works on, e.g. `docker run postgres:17`.
# That socket is owned by a group whose GID comes from the host and rarely lines
# up with a group in this image, so the codespace user otherwise gets "permission
# denied". Align a group to the socket's GID and add the agent to it; a later
# `docker exec -u codespace` resolves this membership from /etc/group. We talk to
# the mounted daemon directly and never start dockerd in here, which cannot bind
# the bind-mounted socket anyway ("device or resource busy").
DOCKER_SOCK=/var/run/docker.sock
if [ -S "${DOCKER_SOCK}" ]; then
  sock_gid="$(stat -c '%g' "${DOCKER_SOCK}")"
  group_name="$(getent group "${sock_gid}" | cut -d: -f1 || true)"
  if [ -z "${group_name}" ]; then
    group_name=docker-host
    groupadd -g "${sock_gid}" "${group_name}"
  fi
  usermod -aG "${group_name}" "${AGENT_USER}"
fi

# Hand off to the idle command (tail -f /dev/null).
exec "$@"
