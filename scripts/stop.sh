#!/usr/bin/env bash
# Stop the Seraphim stack, leaving volumes (Postgres data, agent workspace,
# Tailscale state) intact so nothing is lost.
set -euo pipefail

cd "$(dirname "$0")/.."

docker compose down
