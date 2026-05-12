#!/usr/bin/env bash
#
# Purpose: Prepare a Linux backup host (NAS, home server, or spare laptop) to store Backr snapshots.
# Role: Distro-aware install of OpenSSH server + rsync, enables sshd, drops in PubkeyAuthentication yes,
#       then creates a dedicated UNIX user, backup root directory, and ~/.ssh layout for pubkey SSH.
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

#
# Echoes package backend: apt, dnf, yum, pacman, zypper, apk, or unknown (reads /etc/os-release).
#
detect_pkg_backend() {
  if [[ ! -f /etc/os-release ]]; then
    echo unknown
    return
  fi
  local id id_like
  # shellcheck source=/dev/null
  . /etc/os-release
  id="${ID:-}"
  id_like="${ID_LIKE:-}"

  case "$id" in
    debian | ubuntu | linuxmint | pop | zorin | elementary | raspbian | kali)
      echo apt
      ;;
    fedora | rhel | centos | almalinux | rocky | ol)
      echo dnf
      ;;
    amzn)
      if command -v dnf &>/dev/null; then
        echo dnf
      elif command -v yum &>/dev/null; then
        echo yum
      else
        echo unknown
      fi
      ;;
    arch | manjaro | cachyos | endeavouros | garuda)
      echo pacman
      ;;
    opensuse-tumbleweed | opensuse-leap | sled | sles | opensuse)
      echo zypper
      ;;
    alpine)
      echo apk
      ;;
    *)
      if [[ "$id_like" == *debian* ]] || [[ "$id_like" == *ubuntu* ]]; then
        echo apt
      elif [[ "$id_like" == *fedora* ]] || [[ "$id_like" == *rhel* ]]; then
        echo dnf
      elif [[ "$id_like" == *arch* ]]; then
        echo pacman
      elif [[ "$id_like" == *suse* ]]; then
        echo zypper
      elif [[ "$id_like" == *alpine* ]]; then
        echo apk
      else
        echo unknown
      fi
      ;;
  esac
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

#
# Inputs: $1 dry-run flag (1 = print only). Must run as root.
#
install_server_ssh_rsync() {
  local dry="${1:-0}"
  local backend
  backend="$(detect_pkg_backend)"
  echo "Detected package backend: ${backend}"

  local run
  if [[ "$dry" -eq 1 ]]; then
    run() {
      echo "[dry-run] $*"
    }
  else
    run() {
      "$@"
    }
  fi

  case "$backend" in
    apt)
      export DEBIAN_FRONTEND=noninteractive
      run apt-get update -qq
      run apt-get install -y openssh-server rsync
      ;;
    dnf)
      run dnf install -y openssh-server rsync
      ;;
    yum)
      run yum install -y openssh-server rsync
      ;;
    pacman)
      run pacman -Sy --noconfirm openssh rsync
      ;;
    zypper)
      run zypper --non-interactive refresh
      run zypper --non-interactive install -y openssh rsync
      ;;
    apk)
      run apk update
      run apk add --no-cache openssh rsync
      ;;
    *)
      die "unsupported distro for automatic install — install openssh-server + rsync manually"
      ;;
  esac

  [[ "$dry" -eq 1 ]] && return 0

  # External: systemctl manages SSH units on systemd desktops/servers.
  if command -v systemctl &>/dev/null; then
    if systemctl list-unit-files ssh.service &>/dev/null && systemctl cat ssh.service &>/dev/null; then
      systemctl enable --now ssh
    elif systemctl cat sshd.service &>/dev/null 2>&1; then
      systemctl enable --now sshd
    fi
  fi

  # External: OpenRC on Alpine-style hosts.
  if command -v rc-update &>/dev/null; then
    ssh-keygen -A 2>/dev/null || true
    rc-update add sshd default 2>/dev/null || true
    rc-service sshd start 2>/dev/null || true
  elif ! systemctl is-active --quiet ssh 2>/dev/null && ! systemctl is-active --quiet sshd 2>/dev/null; then
    echo "warning: enable and start sshd manually if this host should accept backups" >&2
  fi
}

#
# Inputs: $1 dry-run flag (1 = print only).
#
sshd_ensure_pubkey_auth() {
  local dry="${1:-0}"
  local drop_in="/etc/ssh/sshd_config.d/99-backr.conf"
  local line='PubkeyAuthentication yes'

  if [[ "$dry" -eq 1 ]]; then
    echo "[dry-run] mkdir -p /etc/ssh/sshd_config.d"
    echo "[dry-run] echo '${line}' > '${drop_in}'"
    echo "[dry-run] systemctl reload ssh || systemctl reload sshd || rc-service sshd reload"
    return 0
  fi

  mkdir -p /etc/ssh/sshd_config.d
  if [[ ! -f "$drop_in" ]] || ! grep -qxF "$line" "$drop_in" 2>/dev/null; then
    printf '%s\n' "$line" >"$drop_in"
    chmod 644 "$drop_in"
  fi

  if systemctl cat ssh.service &>/dev/null 2>&1; then
    systemctl reload ssh 2>/dev/null || true
  fi
  if systemctl cat sshd.service &>/dev/null 2>&1; then
    systemctl reload sshd 2>/dev/null || true
  fi
  if command -v rc-service &>/dev/null; then
    rc-service sshd reload 2>/dev/null || rc-service sshd restart 2>/dev/null || true
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

#
# Writes `/etc/backr/host.toml` so Backr can open host-dashboard mode on this machine without a client config.
#
# Inputs: uses globals BACKR_ROOT, BACKR_USER, DRY_RUN, run_cmd pattern via explicit branches.
#
write_host_marker() {
  local meta_dir="/etc/backr"
  local f="${meta_dir}/host.toml"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] mkdir -p ${meta_dir} && write ${f}"
    return 0
  fi
  mkdir -p "$meta_dir"
  chmod 755 "$meta_dir"
  printf '%s\n' "backup_root = \"${BACKR_ROOT}\"" "ssh_user = \"${BACKR_USER}\"" >"$f"
  chmod 644 "$f"
}

#
# Prints a single completion line (no follow-up checklist — setup is fully automated).
#
print_host_ready() {
  echo "Backr backup host ready (backup_root=${BACKR_ROOT}, ssh_user=${BACKR_USER})."
}

main() {
  parse_args "$@"
  require_linux_root
  normalize_root
  install_server_ssh_rsync "$DRY_RUN"
  sshd_ensure_pubkey_auth "$DRY_RUN"
  ensure_user_exists
  ensure_backup_tree
  ensure_ssh_dir
  write_host_marker
  print_host_ready
}

main "$@"
