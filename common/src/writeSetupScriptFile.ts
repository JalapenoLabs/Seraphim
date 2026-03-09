// Copyright © 2026 Jalapeno Labs

/* eslint-disable max-len */

import type { TaskWithFullContext } from './types'

// Core
import { writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { Cloner } from './cloning/polymorphism/cloner'

// Misc
import { redactSecrets } from './redactSecrets'
import {
  DOCKER_WORKDIR,
  SETUP_SCRIPT_NAME,
  BACKUP_GITHUB_CLONE_SAMPLE_URL,
  SETUP_SUCCESS_LINE,
  SETUP_FAILURE_LINE,
} from './constants'

export async function writeSetupScriptFile(
  contextDir: string,
  task: TaskWithFullContext,
  cloner: Cloner = new Cloner(BACKUP_GITHUB_CLONE_SAMPLE_URL),
  secrets: string[] = [],
) {
  const gitCloneUrl = cloner.getCloneUrl()

  const gitUserName = task.gitAccount?.name || 'codex'
  const gitUserEmail = task.gitAccount?.email || 'codex@jalapenolabs.io'
  const gitSourceBranch = task.sourceGitBranch || 'main'
  const gitWorkBranchName = `seraphim-work`

  let customUserSetupCommands = task.workspace.setupScript?.trim() ?? ''
  if (customUserSetupCommands) {
    let remappedCommands = ''
    for (const line of customUserSetupCommands.split('\n')) {
      const escaped = line.replaceAll('\'', `'\\''`) // produces: '\''
      const safe = redactSecrets(escaped, secrets)
      remappedCommands += `printf '%s\\n' '$ ${safe}'\n${line}\n`
    }

    customUserSetupCommands = `
printf "%b\n" "$\{BOLD}$\{CYAN}======== RUNNING CUSTOM SETUP SCRIPT ========$\{RESET}"

${remappedCommands}

printf "%b\n" "$\{BOLD}$\{GREEN}======== FINISHED CUSTOM SETUP SCRIPT ========$\{RESET}"
    `.trim()
  }

  let authCommand = ''
  if (cloner.token) {
    const encodedToken = encodeURIComponent(cloner.token)
    const githubTokenBaseUrl = `https://x-access-token:${encodedToken}@github.com/`

    authCommand = `
printf "https://x-access-token:%s@github.com\n" "${encodedToken}" > /root/.git-credentials

git config --global --unset-all url."${githubTokenBaseUrl}".insteadOf || true
git config --global --add url."${githubTokenBaseUrl}".insteadOf "https://github.com/"
git config --global --add url."${githubTokenBaseUrl}".insteadOf "git@github.com:"
git config --global --add url."${githubTokenBaseUrl}".insteadOf "ssh://git@github.com/"

printf "Setting up Github CLI authentication...\\n"
echo '${cloner.token}' | gh auth login --with-token
    `.trim()
  }

  const updatedContent = `
#!/usr/bin/env bash
set -euo pipefail

RESET='\\033[0m'
BOLD='\\033[1m'
CYAN='\\033[36m'
RED='\\033[31m'
GREEN='\\033[32m'
GRAY='\\033[90m'

export HOME=/root
export XDG_CONFIG_HOME=/root/.config
export GIT_CONFIG_GLOBAL=/root/.gitconfig

export FORCE_COLOR=1

export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
export YARN_ENABLE_PROGRESS_BARS=0
export YARN_ENABLE_TELEMETRY=0

unset GIT_CONFIG GIT_CONFIG_COUNT || true

mkdir -p "$HOME" "$XDG_CONFIG_HOME"
touch "$GIT_CONFIG_GLOBAL"

# Merge stderr into stdout for the setup block: '{ ... } 2>&1'
# This is to enforce logging in the setup stuff is intrepeted in the same order for logging without TTY attached
# Without this, logs get "mixed up" and stderr might be logged on the container out of order.
{
  trap 'printf "$\{BOLD}$\{RED}${SETUP_FAILURE_LINE} (exit $?)$\{RESET}\n"; echo; touch /opt/seraphim/setup-failed; exit 1' ERR

  cd ${DOCKER_WORKDIR}

  # This removes non-hidden + hidden (excluding . and ..), and || true prevents an empty dir from killing the script.
  # We always ensure the workdir is clear before cloning, otherwise 'git clone .' will fail
  find . -mindepth 1 -maxdepth 1 -exec rm -rf -- {} + || true

  printf "%b\n" "$\{BOLD}$\{CYAN}======== CLONING REPOSITORY ========$\{RESET}"

  echo "$ git version"
  git --version || true

  # Setup git user
  git config --global user.name  "${gitUserName}"
  git config --global user.email "${gitUserEmail}"

  # Setup git credential helpers
  git config --global credential.helper store
  git config --global --unset-all url."https://github.com/".insteadOf || true
  git config --global --add url."https://github.com/".insteadOf "git@github.com:"
  git config --global --add url."https://github.com/".insteadOf "ssh://git@github.com/"

  # Setup your branch to support 'git pull' and 'git push' shorthand
  git config --global pull.default current
  git config --global push.default current

  # Setup quality of life defaults:
  git config --global color.ui auto
  git config --global pull.rebase true
  git config --global init.defaultbranch main

  # Make first push automatically set the upstream
  git config --global push.autoSetupRemote true
  git config --global remote.pushDefault origin

  ${authCommand}

  echo "$ git clone --branch \\"${gitSourceBranch}\\" \\"${redactSecrets(gitCloneUrl, secrets)}\\" ."
  git clone --branch "${gitSourceBranch}" "${gitCloneUrl}" .

  echo 'Cloning submodules:'
  printf "$ git submodule sync --recursive\n"
  git submodule sync --recursive
  printf "$ git submodule update --init --recursive\n"
  git submodule update --init --recursive

  echo "Git clone complete."

  echo "$ git switch -C "${gitWorkBranchName}" "origin/${gitSourceBranch}""
  git switch -C "${gitWorkBranchName}" "origin/${gitSourceBranch}"

  #ls -1AF --group-directories-first
  echo "Cloned workspace files:"
  (cd / && tree -aF -L 1 ${DOCKER_WORKDIR})

  printf "%b\n" "$\{BOLD}$\{GREEN}======== FINISHED CLONING ========$\{RESET}"

  ${customUserSetupCommands}

  # CD again b/c the custom user commands might have changed the directory, but we want to ensure we're in the workspace at the end of the setup script
  cd ${DOCKER_WORKDIR}

  touch /opt/seraphim/setup-success
  printf "%b\n" "$\{BOLD}$\{GREEN}${SETUP_SUCCESS_LINE}$\{RESET}"
} 2>&1

echo "Codex version: $(codex --version)"
echo "Codex bin: $(which codex || true)"

# Idle indefinitely on the main process
tail -f /dev/null
  `

  const scriptPath = resolve(contextDir, SETUP_SCRIPT_NAME)
  await writeFile(scriptPath, updatedContent, 'utf8')

  return SETUP_SCRIPT_NAME
}
