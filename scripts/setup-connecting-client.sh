#!/usr/bin/env bash
#
# Purpose: Prepare a Linux laptop (or workstation) that runs the Backr app to push backups over SSH/rsync.
# Role: Installs OpenSSH client + rsync when missing (apt/dnf/yum/pacman/zypper/apk, using sudo), ensures a
#       projects directory exists, and optionally creates an SSH key pair.
#
# Usage:
#   ./scripts/setup-connecting-client.sh [options]
#
# Options:
#   --projects-dir PATH   Local folder containing one subdirectory per project (default: ~/Projects).
#   --skip-keygen         Do not offer to create ~/.ssh/id_ed25519 if missing.
#   -h, --help            Show this text.

set -euo pipefail

_BACKR_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# External: `source` loads `backr_install_client_ssh_rsync` and related helpers (see file header there).
# shellcheck source=lib/linux_pkg_install.inc.sh
source "${_BACKR_SCRIPT_DIR}/lib/linux_pkg_install.inc.sh"

PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
SKIP_KEYGEN=0

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '1,18p' "$0" | tail -n +2
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --projects-dir)
        PROJECTS_DIR="${2:-}"
        [[ -n "$PROJECTS_DIR" ]] || die "--projects-dir needs a value"
        shift 2
        ;;
      --skip-keygen)
        SKIP_KEYGEN=1
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

require_linux() {
  [[ "$(uname -s)" == "Linux" ]] ||
    die "this script targets Linux (detected: $(uname -s)); install OpenSSH client and rsync manually elsewhere"
}

expand_projects_dir() {
  PROJECTS_DIR="${PROJECTS_DIR/#\~/$HOME}"
}

# Ensures ssh and rsync exist, calling the distro installer when either is missing.
#
# Inputs: none.
# Outputs: none; exits non-zero if binaries are still missing after install.
ensure_ssh_rsync_available() {
  local missing=()
  command -v ssh &>/dev/null || missing+=(ssh)
  command -v rsync &>/dev/null || missing+=(rsync)
  if [[ "${#missing[@]}" -eq 0 ]]; then
    echo "ssh and rsync are already installed."
    return 0
  fi
  echo "Installing missing tools: ${missing[*]} …"
  backr_install_client_ssh_rsync
  missing=()
  command -v ssh &>/dev/null || missing+=(ssh)
  command -v rsync &>/dev/null || missing+=(rsync)
  [[ "${#missing[@]}" -eq 0 ]] || die "still missing after package install: ${missing[*]}"
}

maybe_create_ssh_key() {
  local priv="$HOME/.ssh/id_ed25519"
  [[ "$SKIP_KEYGEN" -eq 1 ]] && return 0
  if [[ -f "$priv" ]]; then
    echo "SSH private key already present: ${priv}"
    return 0
  fi
  mkdir -p "$HOME/.ssh"
  chmod 700 "$HOME/.ssh"
  echo "No Ed25519 key found at ${priv}."
  read -r -p "Generate one now? [y/N] " ans || true
  if [[ "${ans:-}" =~ ^[yY]$ ]]; then
    # External: `ssh-keygen` writes key material under ~/.ssh (OpenSSH).
    ssh-keygen -t ed25519 -f "$priv" -N "" -C "backr-$(whoami)@$(hostname -s 2>/dev/null || echo host)"
    echo "Created ${priv} and ${priv}.pub"
  else
    echo "Skipping key generation; create a key manually before using Backr."
  fi
}

print_next_steps() {
  cat <<EOF

Local Backr settings (after you finish the in-app wizard):
  [local]
  projects_path = "${PROJECTS_DIR}"

Copy your public key to the backup host (replace HOST):
  ssh-copy-id -i ~/.ssh/id_ed25519.pub backr@HOST

Then in Backr set [remote].ssh_key to your private key path, e.g.:
  ssh_key = "${HOME}/.ssh/id_ed25519"

Build/run this repo:
  npm install
  npm run tauri:dev        # development
  npm run tauri:build      # release bundle

EOF
}

ensure_projects_dir() {
  if [[ ! -d "$PROJECTS_DIR" ]]; then
    echo "Creating projects directory: ${PROJECTS_DIR}"
    mkdir -p "$PROJECTS_DIR"
  else
    echo "Projects directory already exists: ${PROJECTS_DIR}"
  fi
}

main() {
  parse_args "$@"
  require_linux
  expand_projects_dir
  ensure_ssh_rsync_available
  ensure_projects_dir
  maybe_create_ssh_key
  print_next_steps
}

main "$@"
