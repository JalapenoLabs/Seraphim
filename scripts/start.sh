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

docker compose up -d --build
docker compose ps
