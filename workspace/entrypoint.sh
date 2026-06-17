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

# --- Make documented ~/.claude/... paths resolve (issue #259) ----------------
# The global instructions tell the agent to read ~/.claude/orgs/*.md and every
# applicable ~/.claude/docs/*.md before its first edit (and to HALT if the org
# file is missing). But the config repo lives under CLAUDE_CONFIG_DIR
# (/workspace/.claude) while the agent's $HOME is /home/codespace, so those
# literal home-dir paths don't resolve and can trigger a false halt that wastes a
# run. Symlink the agent's ~/.claude at the real config dir so the documented
# paths resolve, with no other effect.
#
# We deliberately do NOT move $HOME to /workspace (the alternative considered in
# the issue): SSH keys are copied to ${AGENT_HOME}/.ssh below and `gh auth
# setup-git` wires git credentials under ${AGENT_HOME}, so relocating $HOME would
# break SSH cloning and auth unless those were relocated too. The symlink has no
# such side effects.
#
# If a real (non-symlink) ~/.claude already exists, link only the two documented
# subdirs so we never clobber other Claude state the home dir may hold. A symlink
# to a target that the API clones later (orgs/, docs/) is fine: it dangles until
# the config repo lands, then resolves.
if [ "${CLAUDE_CONFIG_DIR}" != "${AGENT_HOME}/.claude" ]; then
  if [ ! -e "${AGENT_HOME}/.claude" ] || [ -L "${AGENT_HOME}/.claude" ]; then
    ln -sfn "${CLAUDE_CONFIG_DIR}" "${AGENT_HOME}/.claude"
  else
    ln -sfn "${CLAUDE_CONFIG_DIR}/orgs" "${AGENT_HOME}/.claude/orgs"
    ln -sfn "${CLAUDE_CONFIG_DIR}/docs" "${AGENT_HOME}/.claude/docs"
  fi
  chown -h "${AGENT_USER}:${AGENT_USER}" "${AGENT_HOME}/.claude" 2>/dev/null || true
fi

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

# --- Git identity so every cloned repo can commit (issue #214) ----------------
# Without this the agent's `git commit` fails with "Author identity unknown" in any
# repo that lacks a local user.*; only the per-repo case was ever set before. Write
# it SYSTEM-wide (/etc/gitconfig) so it covers every user and every `docker exec`,
# across all the flat clones under /workspace, with no per-repo setup. The values
# are deployment-specific (config, not code): supplied via `.env` (GIT_USER_NAME /
# GIT_USER_EMAIL), with a safe fallback so a commit never hard-fails when unset. A
# repo-local `user.*` still overrides this when a task needs a different identity.
git config --system user.name "${GIT_USER_NAME:-Seraphim}"
git config --system user.email "${GIT_USER_EMAIL:-seraphim@users.noreply.github.com}"

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
