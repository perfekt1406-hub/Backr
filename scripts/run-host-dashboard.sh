#!/usr/bin/env bash
#
# Purpose: Launch Backr on a backup host with explicit host-dashboard IPC mode.
# Role: Sets BACKR_HOST_MODE so bootstrap uses BACKR_HOST_BACKUP_ROOT or `/etc/backr/host.toml`.
#
# Usage: from repo root after `./scripts/setup-connecting-client.sh` on this machine:
#   ./scripts/run-host-dashboard.sh
#

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export BACKR_HOST_MODE="${BACKR_HOST_MODE:-1}"
exec npm run tauri:dev
