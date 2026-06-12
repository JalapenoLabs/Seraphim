#!/usr/bin/env bash
# Bring the whole Seraphim stack up in the background.
#
# Convenience wrapper around `docker compose up -d`. On Windows, run this from
# Git Bash or WSL; the underlying compose stack is all Linux containers.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ ! -f .env ]]; then
  echo "No .env found. Copy .env.example to .env and fill it in first." >&2
  exit 1
fi

# Stamp the running build with the host's commit/branch (for the in-app update
# check) and tell the API where the repo lives on the host (so its self-updater
# can git pull + rebuild). HOST_REPO_DIR can be overridden in .env (e.g. on
# Windows, set it to a Docker-friendly path).
export GIT_SHA="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
export GIT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"
export HOST_REPO_DIR="${HOST_REPO_DIR:-$(pwd)}"

docker compose up -d --build
docker compose ps
