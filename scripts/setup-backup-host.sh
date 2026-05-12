#!/usr/bin/env bash
#
# Purpose: Prepare a Linux backup host (NAS, home server, or spare laptop) to store Backr snapshots.
# Role: Creates a dedicated UNIX user, backup root directory, and ~/.ssh layout for pubkey-only SSH.
#
# Run with sudo on the machine that will receive rsync over SSH. Non-Linux hosts are not supported here.
#
# Usage:
#   sudo ./scripts/setup-backup-host.sh [options]
#
# Options:
#   --user NAME       Dedicated account (default: backr).
#   --root PATH       Absolute backup root on disk (default: /srv/backr).
#   --dry-run         Print actions only.
#   -h, --help        Show this text.

set -euo pipefail

BACKR_USER="${BACKR_USER:-backr}"
BACKR_ROOT="${BACKR_ROOT:-/srv/backr}"
DRY_RUN=0

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '1,20p' "$0" | tail -n +2
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --user)
        BACKR_USER="${2:-}"
        [[ -n "$BACKR_USER" ]] || die "--user needs a value"
        shift 2
        ;;
      --root)
        BACKR_ROOT="${2:-}"
        [[ -n "$BACKR_ROOT" ]] || die "--root needs a value"
        shift 2
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        die "unknown argument: $1 (try --help)"
        ;;
    esac
  done
}

require_linux_root() {
  [[ "$(uname -s)" == "Linux" ]] || die "this host setup script targets Linux only"
  [[ "${EUID:-0}" -eq 0 ]] || die "run as root (sudo) so we can create the backup user and ${BACKR_ROOT}"
}

normalize_root() {
  BACKR_ROOT="${BACKR_ROOT%/}"
  [[ "$BACKR_ROOT" == /* ]] || die "--root must be an absolute path, got: ${BACKR_ROOT}"
}

run_cmd() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] $*"
  else
    "$@"
  fi
}

ensure_user_exists() {
  if id "$BACKR_USER" &>/dev/null; then
    echo "User '${BACKR_USER}' already exists."
  else
    echo "Creating user '${BACKR_USER}' (login shell: /bin/bash)."
    run_cmd useradd -m -s /bin/bash "$BACKR_USER"
  fi
}

ensure_backup_tree() {
  echo "Ensuring backup root ${BACKR_ROOT} (owned by ${BACKR_USER})."
  run_cmd mkdir -p "$BACKR_ROOT"
  run_cmd chown "${BACKR_USER}:${BACKR_USER}" "$BACKR_ROOT"
  run_cmd chmod 755 "$BACKR_ROOT"
}

ensure_ssh_dir() {
  local home_dir
  home_dir="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  [[ -n "$home_dir" ]] || die "could not resolve home for ${BACKR_USER}"

  echo "Ensuring ${home_dir}/.ssh for authorized_keys."
  run_cmd mkdir -p "${home_dir}/.ssh"
  run_cmd chown "${BACKR_USER}:${BACKR_USER}" "${home_dir}/.ssh"
  run_cmd chmod 700 "${home_dir}/.ssh"
  if [[ ! -f "${home_dir}/.ssh/authorized_keys" ]]; then
    run_cmd touch "${home_dir}/.ssh/authorized_keys"
    run_cmd chown "${BACKR_USER}:${BACKR_USER}" "${home_dir}/.ssh/authorized_keys"
    run_cmd chmod 600 "${home_dir}/.ssh/authorized_keys"
  fi
}

print_sshd_hints() {
  cat <<EOF

Next steps on this machine:
  1. Install / enable SSH if needed, e.g. on Debian/Ubuntu:
       sudo apt-get update && sudo apt-get install -y openssh-server
       sudo systemctl enable --now ssh
  2. In /etc/ssh/sshd_config ensure pubkey auth is allowed:
       PubkeyAuthentication yes
     Then: sudo systemctl reload ssh  (or sshd)
  3. From each laptop, install your public key, e.g.:
       ssh-copy-id -i ~/.ssh/id_ed25519.pub ${BACKR_USER}@$(hostname -f 2>/dev/null || hostname -s)

Backr config.toml should use:
  [remote]
  host        = "<this host LAN IP or DNS>"
  user        = "${BACKR_USER}"
  ssh_key     = "<path to your PRIVATE key on the laptop>"
  port        = 22
  backup_path = "${BACKR_ROOT}"

Snapshot trees will appear as: ${BACKR_ROOT}/<project>/<YYYY-MM-DD_HH-MM-SS>/
EOF
}

main() {
  parse_args "$@"
  require_linux_root
  normalize_root
  ensure_user_exists
  ensure_backup_tree
  ensure_ssh_dir
  print_sshd_hints
}

main "$@"
