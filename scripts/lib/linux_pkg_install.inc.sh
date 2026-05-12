#!/usr/bin/env bash
#
# Purpose: Distro-aware installation of OpenSSH and rsync for Backr setup scripts.
# Role: Sourced by setup-connecting-client.sh (client) and setup-backup-host.sh (server).
#       Not intended to be executed directly.
#

# Echoes the package backend identifier: apt, dnf, pacman, zypper, apk, or unknown.
#
# Inputs: none (reads /etc/os-release).
# Outputs: single token on stdout.
backr_detect_pkg_backend() {
  if [[ ! -f /etc/os-release ]]; then
    echo unknown
    return
  fi
  # External: `. /etc/os-release` sets ID and ID_LIKE for distro detection.
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

# Runs a command as root, or via sudo when the invoking user is non-root.
#
# Inputs: command plus arguments.
# Outputs: inherits callee stdout/stderr; exits on failure.
backr_run_privileged() {
  if [[ "${EUID:-0}" -eq 0 ]]; then
    "$@"
  elif command -v sudo &>/dev/null; then
    sudo "$@"
  else
    echo "error: need root or sudo to install packages (install sudo or run as root)" >&2
    return 1
  fi
}

# Installs OpenSSH client tooling and rsync for the machine that runs Backr.
#
# Inputs: none.
# Outputs: none; exits non-zero if the distro is unsupported or package install fails.
backr_install_client_ssh_rsync() {
  local backend
  backend="$(backr_detect_pkg_backend)"
  echo "Detected package backend: ${backend}"

  case "$backend" in
    apt)
      export DEBIAN_FRONTEND=noninteractive
      backr_run_privileged apt-get update -qq
      backr_run_privileged apt-get install -y openssh-client rsync
      ;;
    dnf)
      backr_run_privileged dnf install -y openssh-clients rsync
      ;;
    yum)
      backr_run_privileged yum install -y openssh-clients rsync
      ;;
    pacman)
      backr_run_privileged pacman -Sy --noconfirm openssh rsync
      ;;
    zypper)
      backr_run_privileged zypper --non-interactive refresh
      backr_run_privileged zypper --non-interactive install -y openssh rsync
      ;;
    apk)
      backr_run_privileged apk update
      backr_run_privileged apk add --no-cache openssh-client rsync
      ;;
    *)
      echo "error: unsupported Linux distro for automatic package install (need ssh + rsync packages)" >&2
      echo "Install OpenSSH client and rsync manually, then re-run this script." >&2
      return 1
      ;;
  esac
}

# Installs OpenSSH server and rsync on the backup host and tries to start sshd.
#
# Inputs:
#   $1 — optional dry-run flag; when "1", only print what would run (caller must be root).
# Outputs: none.
backr_install_server_ssh_rsync() {
  local dry="${1:-0}"
  local backend
  backend="$(backr_detect_pkg_backend)"
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
      echo "error: unsupported Linux distro for automatic package install (need sshd + rsync)" >&2
      return 1
      ;;
  esac

  [[ "$dry" -eq 1 ]] && return 0

  # External: systemctl enables and starts the platform SSH unit (ssh vs sshd).
  if command -v systemctl &>/dev/null; then
    if systemctl list-unit-files ssh.service &>/dev/null && systemctl cat ssh.service &>/dev/null; then
      systemctl enable --now ssh
    elif systemctl cat sshd.service &>/dev/null 2>&1; then
      systemctl enable --now sshd
    fi
  fi

  # Alpine / OpenRC-style hosts (no systemd).
  if command -v rc-update &>/dev/null; then
    ssh-keygen -A 2>/dev/null || true
    rc-update add sshd default 2>/dev/null || true
    rc-service sshd start 2>/dev/null || true
  elif ! systemctl is-active --quiet ssh 2>/dev/null && ! systemctl is-active --quiet sshd 2>/dev/null; then
    echo "warning: enable and start sshd manually if this host should accept backups" >&2
  fi
}

# Ensures PubkeyAuthentication is enabled via sshd drop-in and reloads the daemon.
#
# Inputs:
#   $1 — dry-run flag "1" to print only.
# Outputs: none.
backr_sshd_ensure_pubkey_auth() {
  local dry="${1:-0}"
  local drop_in="/etc/ssh/sshd_config.d/99-backr.conf"
  local line='PubkeyAuthentication yes'

  if [[ "$dry" -eq 1 ]]; then
    echo "[dry-run] mkdir -p /etc/ssh/sshd_config.d"
    echo "[dry-run] echo '${line}' > '${drop_in}'"
    echo "[dry-run] systemctl reload ssh || systemctl reload sshd"
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
