#!/usr/bin/env bash
# Restart the Seraphim stack without tearing down volumes.
set -euo pipefail

cd "$(dirname "$0")/.."

docker compose restart
docker compose ps
