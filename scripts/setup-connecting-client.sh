#!/usr/bin/env bash
#
# Purpose: Prepare a Linux machine for Backr — by default installs deps, builds the desktop AppImage from this repo,
#          registers it under ~/.local/share (launcher menu entry), and optionally walks through a short questionnaire so
#          «unknown» networking/server facts become concrete next-step instructions at the end.
# Role: Distro-aware OS packages for Tauri (WebKitGTK, SSL, build tools), Node.js LTS,
#       Rust via rustup (respecting src-tauri/Cargo.toml rust-version), OpenSSH client + rsync, git/curl;
#       npm ci/npm install; optional projects dir + SSH key; npm run tauri:build + AppImage install unless --deps-only;
#       optional interactive questionnaire (installs dialog/whiptail-style TUI when missing for arrow-key menus) +
#       tailored «next steps» when /dev/tty exists.
#
# Run from anywhere with sudo available when elevated installs are needed:
#   ./scripts/setup-connecting-client.sh [options]
#
# Options:
#   --projects-dir PATH            Local folder containing one subdirectory per project (default: ~/Projects).
#   --skip-keygen                  Do not offer to create ~/.ssh/id_ed25519 if missing.
#   --backup-host TARGET           After setup: probe pubkey SSH to TARGET (host/IP or user@host; default user backr).
#                                  If login fails, prints your pubkey and the exact BACKR_AUTHORIZED_KEYS curl line — no passwords.
#   --deps-only                    Install toolchain and npm deps only (no AppImage build / menu install); use for dev.
#   --install-appimage             Same as default (explicit): build AppImage locally and install launcher entry.
#   --install-appimage-build       Same as default (explicit).
#   --appimage-url URL             Download this AppImage and add launcher entry only (no compile).
#   --non-interactive              Skip questionnaire and abbreviated default next-steps (CI / pipes).
#   -h, --help                     Show this text.
#
# Environment:
#   BACKR_BACKUP_HOST       Same as --backup-host (e.g. backr@192.168.1.10 or 192.168.1.10).
#   BACKR_NON_INTERACTIVE=1 Same as --non-interactive.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
SKIP_KEYGEN=0
# Exclusive setup goal: build (default) | deps | download — see set_setup_kind().
SETUP_KIND=""
APPIMAGE_URL_OVERRIDE=""
# Optional backup SSH target for pubkey probe / bootstrap hints (see verify_pubkey_ssh_or_print_bootstrap_line).
BACKUP_SSH_TARGET=""
BACKR_NON_INTERACTIVE="${BACKR_NON_INTERACTIVE:-0}"
SURVEY_SKIP_NO_TTY=0
SURVEY_CLIENT_NETWORK="${SURVEY_CLIENT_NETWORK:-unknown}"
SURVEY_CLIENT_SERVER_READY="${SURVEY_CLIENT_SERVER_READY:-unknown}"
SURVEY_CLIENT_SSH_PORT="${SURVEY_CLIENT_SSH_PORT:-unknown}"
SURVEY_CLIENT_SSH_CUSTOM_PORT="${SURVEY_CLIENT_SSH_CUSTOM_PORT:-}"
SURVEY_CLIENT_HOST_PLAN="${SURVEY_CLIENT_HOST_PLAN:-unknown}"

APT_UPDATED=0

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '1,28p' "$0" | tail -n +2
}

#
# Inputs: one of build | deps | download.
# Outputs: sets SETUP_KIND or dies if another mode was already chosen.
#
set_setup_kind() {
  local k="$1"
  if [[ -z "$SETUP_KIND" ]]; then
    SETUP_KIND="$k"
    return 0
  fi
  [[ "$SETUP_KIND" == "$k" ]] ||
    die "conflicting options — use only one of: (default / --install-appimage / --install-appimage-build), --deps-only, or --appimage-url"
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
      --backup-host)
        BACKUP_SSH_TARGET="${2:-}"
        [[ -n "$BACKUP_SSH_TARGET" ]] || die "--backup-host needs a value"
        shift 2
        ;;
      --deps-only)
        set_setup_kind deps
        shift
        ;;
      --install-appimage)
        set_setup_kind build
        shift
        ;;
      --install-appimage-build)
        set_setup_kind build
        shift
        ;;
      --appimage-url)
        APPIMAGE_URL_OVERRIDE="${2:-}"
        [[ -n "$APPIMAGE_URL_OVERRIDE" ]] || die "--appimage-url needs a value"
        set_setup_kind download
        shift 2
        ;;
      --non-interactive)
        BACKR_NON_INTERACTIVE=1
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
# Prints lines to /dev/tty when stdin may not be interactive (e.g. nested tooling).
#
survey_print_tty_client() {
  printf '%s\n' "$@" >/dev/tty
}

#
# Inputs: none (uses detect_pkg_backend + run_privileged). Outputs: installs dialog when missing on supported Linux distros;
#         silent no-op when dialog/whiptail already present; returns non-zero on failure (caller falls back to typed menu).
# External: apt-get/dnf/yum/pacman/zypper/apk — same families as install_connecting_os_packages.
#
ensure_survey_tui_pkg_connecting() {
  command -v dialog &>/dev/null && return 0
  command -v whiptail &>/dev/null && return 0
  [[ "$(uname -s)" == "Linux" ]] || return 1
  local backend
  backend="$(detect_pkg_backend)"
  case "$backend" in
    apt)
      apt_update_once
      run_privileged apt-get install -y dialog || return 1
      ;;
    dnf)
      run_privileged dnf install -y dialog || return 1
      ;;
    yum)
      run_privileged yum install -y dialog || return 1
      ;;
    pacman)
      run_privileged pacman -Sy --noconfirm dialog || return 1
      ;;
    zypper)
      run_privileged zypper --non-interactive refresh
      run_privileged zypper --non-interactive install -y dialog || return 1
      ;;
    apk)
      run_privileged apk update
      run_privileged apk add --no-cache dialog || return 1
      ;;
    *)
      return 1
      ;;
  esac
  command -v dialog &>/dev/null || command -v whiptail &>/dev/null
}

#
# Inputs: $1 question text, $2–$4 three option strings (fourth is always «I don't know»).
# Outputs: single digit 1–4 on stdout (defaults to 4 on cancel/invalid); uses dialog or whiptail on /dev/tty when available.
#
survey_read_menu_4_client() {
  local title="$1" o1="$2" o2="$3" o3="$4"
  local line="" choice="" mh=22 mw=78 ih=10

  if command -v dialog &>/dev/null; then
    choice="$(dialog --stdout --clear --title "Backr setup" --menu "$title" "$mh" "$mw" "$ih" \
      1 "$o1" 2 "$o2" 3 "$o3" 4 "I don't know" \
      </dev/tty 2>/dev/tty)" || choice=""
    choice="$(printf '%s' "$choice" | tr -d '[:space:]')"
    [[ "$choice" =~ ^[1-4]$ ]] || choice=4
    printf '%s' "$choice"
    return 0
  fi

  if command -v whiptail &>/dev/null; then
    export TERM="${TERM:-xterm-256color}"
    choice="$(whiptail --title "Backr setup" --menu "$title" "$mh" "$mw" "$ih" \
      "1" "$o1" "2" "$o2" "3" "$o3" "4" "I don't know" \
      3>&1 1>&2 2>&3 </dev/tty)" || choice=""
    choice="$(printf '%s' "$choice" | tr -d '[:space:]')"
    [[ "$choice" =~ ^[1-4]$ ]] || choice=4
    printf '%s' "$choice"
    return 0
  fi

  survey_print_tty_client ""
  survey_print_tty_client "$title"
  survey_print_tty_client "  1) $o1"
  survey_print_tty_client "  2) $o2"
  survey_print_tty_client "  3) $o3"
  survey_print_tty_client "  4) I don't know"
  survey_print_tty_client "Choice [1-4]:"
  read -r line </dev/tty 2>/dev/null || line=""
  line="$(printf '%s' "$line" | tr -d '[:space:]')"
  [[ "$line" =~ ^[1-4]$ ]] || line=4
  printf '%s' "$line"
}

#
# Outputs: fills SURVEY_CLIENT_*; may set BACKUP_SSH_TARGET when user types host/IP.
#
run_connecting_client_questionnaire() {
  [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] && return 0
  [[ -c /dev/tty ]] || {
    SURVEY_SKIP_NO_TTY=1
    return 0
  }

  local c="" line=""
  survey_print_tty_client ""
  survey_print_tty_client "=== Backr laptop — quick questionnaire ==="
  survey_print_tty_client "Option 4 is always «I don't know» — you’ll get discovery-style instructions at the end."
  survey_print_tty_client ""
  ensure_survey_tui_pkg_connecting 2>/dev/null || true

  c="$(survey_read_menu_4_client \
    "How will this laptop usually reach the backup server?" \
    "Same LAN (home/office Wi‑Fi or Ethernet)" \
    "Over the internet (public hostname/IP or port-forward)" \
    "VPN first, then private addresses")"
  case "$c" in 1) SURVEY_CLIENT_NETWORK=lan ;; 2) SURVEY_CLIENT_NETWORK=internet ;; 3) SURVEY_CLIENT_NETWORK=vpn ;; *) SURVEY_CLIENT_NETWORK=unknown ;; esac

  c="$(survey_read_menu_4_client \
    "Has setup-backup-host.sh already been run successfully on the backup machine?" \
    "Yes" \
    "Not yet" \
    "Someone else manages that server")"
  case "$c" in 1) SURVEY_CLIENT_SERVER_READY=yes ;; 2) SURVEY_CLIENT_SERVER_READY=no ;; 3) SURVEY_CLIENT_SERVER_READY=other ;; *) SURVEY_CLIENT_SERVER_READY=unknown ;; esac

  c="$(survey_read_menu_4_client \
    "Which SSH port does the backup server's sshd listen on (same port you'll use from here)?" \
    "Default 22" \
    "Custom — you'll type the port next" \
    "I'll figure it out after testing connectivity")"
  case "$c" in
    1)
      SURVEY_CLIENT_SSH_PORT=default
      SURVEY_CLIENT_SSH_CUSTOM_PORT=""
      ;;
    2)
      SURVEY_CLIENT_SSH_PORT=custom
      survey_print_tty_client "Enter the SSH TCP port on the backup server:"
      read -r SURVEY_CLIENT_SSH_CUSTOM_PORT </dev/tty 2>/dev/null || SURVEY_CLIENT_SSH_CUSTOM_PORT=""
      SURVEY_CLIENT_SSH_CUSTOM_PORT="${SURVEY_CLIENT_SSH_CUSTOM_PORT//[^0-9]/}"
      [[ -z "$SURVEY_CLIENT_SSH_CUSTOM_PORT" ]] && SURVEY_CLIENT_SSH_PORT=unknown
      ;;
    3 | 4)
      SURVEY_CLIENT_SSH_PORT=unknown
      SURVEY_CLIENT_SSH_CUSTOM_PORT=""
      ;;
    *)
      SURVEY_CLIENT_SSH_PORT=unknown
      SURVEY_CLIENT_SSH_CUSTOM_PORT=""
      ;;
  esac

  c="$(survey_read_menu_4_client \
    "Do you already know the backup SSH target (hostname or IP) for testing?" \
    "Yes — I'll type it now (defaults UNIX user backr if you omit user@)" \
    "I'll use BACKR_BACKUP_HOST or --backup-host on a later run" \
    "Already passed via this command line / env")"
  case "$c" in
    1)
      SURVEY_CLIENT_HOST_PLAN=typed_now
      survey_print_tty_client "Enter backup host (examples: 192.168.1.50 or backr@nas.local):"
      read -r line </dev/tty 2>/dev/null || line=""
      line="$(printf '%s' "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
      [[ -n "$line" ]] && BACKUP_SSH_TARGET="$line"
      ;;
    2)
      SURVEY_CLIENT_HOST_PLAN=defer
      ;;
    3)
      if [[ -n "$BACKUP_SSH_TARGET" ]] || [[ -n "${BACKR_BACKUP_HOST:-}" ]]; then
        SURVEY_CLIENT_HOST_PLAN=cli_ok
      else
        SURVEY_CLIENT_HOST_PLAN=unknown
        survey_print_tty_client "(Nothing set yet — choose 1 or 2 next time, or pass --backup-host now.)"
      fi
      ;;
    *)
      SURVEY_CLIENT_HOST_PLAN=unknown
      ;;
  esac

  survey_print_tty_client ""
  survey_print_tty_client "Thanks — continuing setup…"
  survey_print_tty_client ""
}

#
# Outputs: guidance derived from SURVEY_CLIENT_* plus optional BACKUP_SSH_TARGET state.
#
emit_connecting_client_custom_next_steps() {
  echo ""
  echo "── Your next steps (questionnaire + current CLI/env) ──"

  if [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]]; then
    cat <<'NXT'
• --non-interactive / BACKR_NON_INTERACTIVE skipped the questionnaire.

  Run interactively (./scripts/setup-connecting-client.sh without --non-interactive) for tailored hints.
  Finish backup-host setup first (curl … setup-backup-host.sh), then trust keys via BACKR_AUTHORIZED_KEYS pattern.

NXT
    return 0
  fi

  if [[ "${SURVEY_SKIP_NO_TTY:-0}" == "1" ]]; then
    cat <<'NXT'
• No /dev/tty — questionnaire skipped (some CI/GUI terminals). Re-run in a normal terminal for prompts.

NXT
  fi

  case "${SURVEY_CLIENT_SERVER_READY:-unknown}" in
    no | unknown)
      cat <<'NXT'
• Backup server prep unclear / not done: on that machine run the hosted setup-backup-host.sh (see README §5) before expecting SSH/rsync from Backr.

NXT
      ;;
  esac

  case "${SURVEY_CLIENT_NETWORK:-unknown}" in
    unknown)
      cat <<'NXT'
• Network path unclear: from this laptop run ping/traceroute to the backup IP; confirm whether you need VPN or port-forwarding for SSH.

NXT
      ;;
    lan)
      cat <<'NXT'
• LAN usage: use the server's private IP (see router DHCP leases or `hostname -I` on the NAS). Off-site backups won't work until VPN or port-forward exists.

NXT
      ;;
    internet)
      cat <<'NXT'
• Internet path: confirm DNS/DDNS resolves, router forwards TCP to sshd's port, and cloud SGs allow inbound SSH.

NXT
      ;;
    vpn)
      cat <<'NXT'
• VPN path: connect VPN before launching Backr backups; SSH targets are usually RFC1918 addresses.

NXT
      ;;
  esac

  if [[ -z "${BACKUP_SSH_TARGET:-}" ]] && [[ -z "${BACKR_BACKUP_HOST:-}" ]]; then
    case "${SURVEY_CLIENT_HOST_PLAN:-unknown}" in
      defer | unknown | '')
        cat <<'NXT'
• No backup SSH target yet: re-run with `--backup-host HOST` or export BACKR_BACKUP_HOST before setup.

NXT
        ;;
    esac
  fi

  case "${SURVEY_CLIENT_SSH_PORT:-unknown}" in
    unknown)
      cat <<'NXT'
• SSH port unsure: try `ssh -v -p 22 user@host`, then retry other `-p` values; match whatever «sshd Port» reports on the backup host script output.

NXT
      ;;
    custom)
      printf '%s\n' "• Custom SSH port ${SURVEY_CLIENT_SSH_CUSTOM_PORT:-?}: use \`ssh -p ${SURVEY_CLIENT_SSH_CUSTOM_PORT} …\` and the same in Backr's SSH settings after setup."
      ;;
  esac

  echo ""
}

require_linux() {
  [[ "$(uname -s)" == "Linux" ]] ||
    die "this script targets Linux desktops only (got $(uname -s))"
}

#
# Outputs package backend: apt|dnf|yum|pacman|zypper|apk|unknown (reads /etc/os-release).
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

#
# Runs argv as root, or via sudo when not root (needs sudo on PATH).
#
run_privileged() {
  if [[ "${EUID:-0}" -eq 0 ]]; then
    "$@"
  elif command -v sudo &>/dev/null; then
    sudo "$@"
  else
    die "need sudo or root to install system packages"
  fi
}

#
# Runs bash reading stdin as root/sudo (for NodeSource and rustup installers).
#
run_stdin_privileged_bash() {
  if [[ "${EUID:-0}" -eq 0 ]]; then
    env DEBIAN_FRONTEND="${DEBIAN_FRONTEND:-noninteractive}" bash "$@"
  elif command -v sudo &>/dev/null; then
    sudo env DEBIAN_FRONTEND="${DEBIAN_FRONTEND:-noninteractive}" bash "$@"
  else
    die "need sudo or root for scripted installers"
  fi
}

#
# Inputs: none — bumps APT_UPDATED once per invocation batch on Debian derivatives.
#
apt_update_once() {
  [[ "$APT_UPDATED" -eq 1 ]] && return 0
  export DEBIAN_FRONTEND=noninteractive
  run_privileged apt-get update -qq
  APT_UPDATED=1
}

#
# Reads rust-version from Cargo.toml (first occurrence).
#
read_cargo_msrv() {
  local f="$REPO_ROOT/src-tauri/Cargo.toml"
  [[ -f "$f" ]] || die "missing $f"
  grep '^rust-version[[:space:]]*=' "$f" | head -1 | sed -n 's/^rust-version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p'
}

#
# Inputs: current rustc semver, minimum required (e.g. 1.77.2). Outputs: true if cur >= min (sort -V).
#
rustc_meets_min() {
  local cur="$1" min="$2"
  [[ -n "$cur" && -n "$min" ]] || return 1
  [[ "$(printf '%s\n%s\n' "$min" "$cur" | sort -V | head -n1)" == "$min" ]]
}

#
# Sources ~/.cargo/env when present and exports PATH so rustc/cargo resolve in this shell.
#
load_cargo_env() {
  export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
  export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
  export PATH="$CARGO_HOME/bin:$PATH"
  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi
}

#
# Installs rustup + stable when rustc missing or older than Cargo.toml rust-version.
#
ensure_rust_toolchain() {
  load_cargo_env
  local msrv
  msrv="$(read_cargo_msrv)"
  [[ -n "$msrv" ]] || msrv="1.77.2"

  local need_install=0
  local cand=""
  if ! command -v rustc &>/dev/null; then
    need_install=1
  else
    cand="$(rustc --version | awk '{print $2}' | cut -d- -f1)"
    if ! rustc_meets_min "$cand" "$msrv"; then
      echo "rustc below rust-version $msrv — refreshing stable toolchain"
      load_cargo_env
      if command -v rustup &>/dev/null; then
        rustup update stable
        load_cargo_env
        cand="$(rustc --version | awk '{print $2}' | cut -d- -f1)"
        rustc_meets_min "$cand" "$msrv" || need_install=1
      else
        need_install=1
      fi
    fi
  fi

  if [[ "$need_install" -eq 1 ]] || ! command -v rustc &>/dev/null; then
    echo "Installing Rust via rustup (https://rustup.rs/) …"
    # External: rustup installer script writes ~/.cargo and adjusts toolchain.
    curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf |
      sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path
    load_cargo_env
  fi

  rustup default stable 2>/dev/null || true
  command -v cargo &>/dev/null || die "cargo missing after rustup — restart shell or source ~/.cargo/env"
  echo "Rust toolchain OK: $(rustc --version)"
}

#
# Ensures Node 18+ and npm using distro packages or NodeSource on Debian/Ubuntu.
#
ensure_nodejs() {
  if command -v node &>/dev/null && command -v npm &>/dev/null; then
    local major
    major="$(node -p 'parseInt(process.versions.node,10)' 2>/dev/null || echo 0)"
    if [[ "${major:-0}" -ge 18 ]]; then
      echo "Node.js OK: $(node --version) / npm $(npm --version)"
      return 0
    fi
    echo "Node.js too old — upgrading install …"
  fi

  local backend
  backend="$(detect_pkg_backend)"
  echo "Installing Node.js (backend: ${backend}) …"

  case "$backend" in
    apt)
      apt_update_once
      run_privileged apt-get install -y ca-certificates curl gnupg
      curl -fsSL https://deb.nodesource.com/setup_22.x | run_stdin_privileged_bash -
      apt_update_once
      run_privileged apt-get install -y nodejs
      ;;
    dnf)
      run_privileged dnf install -y nodejs npm
      ;;
    yum)
      run_privileged yum install -y nodejs npm || die "yum nodejs missing — try Amazon Linux 2023 or install Node manually"
      ;;
    pacman)
      run_privileged pacman -Sy --noconfirm nodejs npm
      ;;
    zypper)
      run_privileged zypper --non-interactive refresh
      run_privileged zypper --non-interactive install -y nodejs22 npm22 2>/dev/null ||
        run_privileged zypper --non-interactive install -y nodejs npm
      ;;
    apk)
      run_privileged apk update
      run_privileged apk add --no-cache nodejs npm
      ;;
    *)
      die "unsupported distro for Node install — see https://nodejs.org/"
      ;;
  esac

  command -v node &>/dev/null && command -v npm &>/dev/null || die "node/npm still missing after install"
  echo "Node.js OK: $(node --version) / npm $(npm --version)"
}

#
# Installs distro packages needed for Tauri desktop builds plus ssh/rsync/git helpers.
#
install_connecting_os_packages() {
  local backend
  backend="$(detect_pkg_backend)"
  echo "Installing OS packages for Backr + Tauri (backend: ${backend}) …"

  case "$backend" in
    apt)
      apt_update_once
      run_privileged apt-get install -y \
        ca-certificates curl wget git gnupg \
        openssh-client rsync \
        build-essential pkg-config cmake \
        libwebkit2gtk-4.1-dev libssl-dev \
        libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev file
      ;;
    dnf)
      run_privileged dnf install -y \
        curl wget git openssl-devel \
        openssh-clients rsync \
        webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel \
        gcc gcc-c++ make cmake pkgconf-pkg-config perl-File-MimeInfo patch
      ;;
    yum)
      run_privileged yum install -y \
        curl wget git openssl-devel \
        openssh-clients rsync \
        gcc gcc-c++ make cmake pkgconfig patch \
        webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel || \
        die "Enterprise/YUM lacks WebKitGTK 4.1 packages — use Fedora/Ubuntu/Arch or install deps manually (see Tauri Linux prerequisites)"
      ;;
    pacman)
      run_privileged pacman -Sy --noconfirm \
        base-devel curl wget git openssl \
        webkit2gtk gtk3 libappindicator-gtk3 librsvg libvips patchelf pkgconf cmake \
        openssh rsync nodejs npm
      ;;
    zypper)
      run_privileged zypper --non-interactive refresh
      run_privileged zypper --non-interactive install -y \
        curl wget git openssl-devel gcc gcc-c++ cmake pkg-config patch \
        webkit2gtk4-devel gtk3-devel libayatana-appindicator-devel librsvg-devel libxdo-devel \
        openssh rsync nodejs npm || \
        run_privileged zypper --non-interactive install -y \
          curl wget git openssl-devel gcc gcc-c++ cmake pkg-config patch \
          webkit2gtk-devel gtk3-devel libappindicator-devel librsvg-devel \
          openssh rsync nodejs npm
      ;;
    apk)
      run_privileged apk update
      run_privileged apk add --no-cache \
        build-base curl wget git openssl-dev pkgconf cmake \
        webkit2gtk-dev gtk+3.0-dev librsvg-dev libayatana-indicator-dev libxscrnsaver-dev \
        openssh-client rsync nodejs npm bash
      ;;
    *)
      die "unsupported distro — install Tauri Linux prerequisites manually: https://v2.tauri.app/start/prerequisites/"
      ;;
  esac
}

#
# Runs npm ci when lockfile exists, otherwise npm install, at repo root.
#
install_node_project_deps() {
  cd "$REPO_ROOT"
  [[ -f package.json ]] || die "missing package.json at $REPO_ROOT"
  echo "Installing npm dependencies in ${REPO_ROOT} …"
  if [[ -f package-lock.json ]]; then
    npm ci || npm install
  else
    npm install
  fi
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
    # External: OpenSSH ssh-keygen writes keys under ~/.ssh.
    ssh-keygen -t ed25519 -f "$priv" -N "" -C "backr-$(whoami)@$(hostname -s 2>/dev/null || echo host)"
    echo "Created ${priv} and ${priv}.pub"
  else
    echo "Skipping key generation; create a key manually before using Backr backups."
  fi
}

ensure_projects_dir() {
  expand_projects_dir
  if [[ ! -d "$PROJECTS_DIR" ]]; then
    echo "Creating projects directory: ${PROJECTS_DIR}"
    mkdir -p "$PROJECTS_DIR"
  else
    echo "Projects directory already exists: ${PROJECTS_DIR}"
  fi
}

expand_projects_dir() {
  PROJECTS_DIR="${PROJECTS_DIR/#\~/$HOME}"
}

#
# Inputs: HTTPS URL to an AppImage.
# Outputs: path to a downloaded temp file (caller must rm); returns non-zero if curl fails.
# External: curl -fL writes bytes to a tempfile (follow redirects).
#
download_appimage_to_tempfile() {
  local url="$1"
  [[ -n "$url" ]] || die "internal: empty AppImage URL"
  command -v curl &>/dev/null || die "curl required to download AppImage"
  local tmp
  tmp="$(mktemp "${TMPDIR:-/tmp}/backr-setup-appimage.XXXXXX")"
  # External: curl fetches URL into tmp with fail-on-HTTP-error and location following.
  if ! curl -fL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 -o "$tmp" "$url"; then
    rm -f "$tmp"
    return 1
  fi
  echo "$tmp"
}

#
# Copies the repo icon into the user icon theme so Icon=com.backr.app resolves in launchers.
#
install_backr_icon_to_user_theme() {
  local src="$REPO_ROOT/src-tauri/icons/128x128.png"
  local dest_dir="$HOME/.local/share/icons/hicolor/128x128/apps"
  local dest="$dest_dir/com.backr.app.png"
  [[ -f "$src" ]] || return 0
  mkdir -p "$dest_dir"
  cp -f "$src" "$dest"
  # External: gtk-update-icon-cache refreshes hicolor index when available (non-fatal if missing).
  if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" &>/dev/null || true
  fi
}

#
# Inputs: absolute path to an AppImage file (already executable).
# Outputs: installs ~/.local/share/backr/Backr.AppImage and ~/.local/share/applications/com.backr.app.desktop.
#
install_appimage_desktop_integration() {
  local src="$1"
  [[ -f "$src" ]] || die "AppImage not found: $src"
  local dest_dir="$HOME/.local/share/backr"
  local dest="${dest_dir}/Backr.AppImage"
  mkdir -p "$dest_dir"
  cp -f "$src" "$dest"
  chmod u+x "$dest" || true

  install_backr_icon_to_user_theme

  local desktop="$HOME/.local/share/applications/com.backr.app.desktop"
  mkdir -p "$(dirname "$desktop")"
  cat >"$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Backr
Comment=Backr desktop backup client
Exec=${dest} %u
Icon=com.backr.app
Terminal=false
Categories=Utility;Archiving;
StartupNotify=true
EOF
  # External: update-desktop-database indexes ~/.local applications for some desktop environments.
  if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$HOME/.local/share/applications" &>/dev/null || true
  fi
  echo "Installed AppImage: ${dest}"
  echo "Launcher entry: ${desktop}"
}

#
# Inputs: APPIMAGE_URL_OVERRIDE must be set (--appimage-url). Downloads that AppImage and installs launcher integration.
# Dies on download failure (does not fall back to a local build).
#
install_appimage_from_network() {
  local url="" tmp=""
  [[ -n "$APPIMAGE_URL_OVERRIDE" ]] || die "internal: install_appimage_from_network needs --appimage-url"
  url="$APPIMAGE_URL_OVERRIDE"
  echo "Using AppImage URL from --appimage-url"
  tmp="$(download_appimage_to_tempfile "$url")" || die "failed to download AppImage"
  trap 'rm -f "$tmp"' RETURN
  install_appimage_desktop_integration "$tmp"
}

#
# Inputs: none (uses REPO_ROOT). Outputs: OS packages, Node, Rust, projects dir, optional SSH key, npm deps, tauri
# release build, then integrates the produced AppImage into ~/.local like install_appimage_desktop_integration.
#
install_appimage_build_and_integrate() {
  install_connecting_os_packages
  ensure_nodejs
  ensure_rust_toolchain
  ensure_projects_dir
  maybe_create_ssh_key
  install_node_project_deps
  echo "Building Backr AppImage (npm run tauri:build) …"
  (cd "$REPO_ROOT" && npm run tauri:build)
  install_appimage_desktop_integration "$(find_built_appimage_path)"
}

#
# Finds the first built *.AppImage under src-tauri/target/release/bundle (after npm run tauri:build).
#
find_built_appimage_path() {
  local hit=""
  hit="$(find "$REPO_ROOT/src-tauri/target/release/bundle" -type f -name '*.AppImage' 2>/dev/null | head -n1 || true)"
  [[ -n "$hit" ]] || die "build produced no .AppImage under src-tauri/target/release/bundle — check tauri bundle targets"
  echo "$hit"
}

#
# Prints post-install hints for AppImage users.
#
print_appimage_done() {
  cat <<EOF

── Backr AppImage installed ──
  Menu / launcher: search for "Backr"
  Binary:          ~/.local/share/backr/Backr.AppImage

EOF
}

print_done() {
  cat <<EOF

── Backr dev environment ready ──
  Repo:        ${REPO_ROOT}
  Projects:    ${PROJECTS_DIR}

Run the app:
  cd ${REPO_ROOT}
  npm run tauri:dev

Release build:
  npm run tauri:build

Backups — trust pubkey on backup host (passwordless; no ssh-copy-id). Use **BACKR_AUTHORIZED_KEYS** on the server
or re-run this script with **--backup-host HOST** to print the one-liner.

Then set in ~/.config/backr/config.toml after the wizard:
  [local]
  projects_path = "${PROJECTS_DIR}"

EOF
}

#
# Inputs: raw TARGET from CLI (hostname, IP, or user@host).
# Outputs: ssh destination string (default UNIX user backr when no @ is present).
#
normalize_backup_ssh_target() {
  local raw="$1"
  [[ -z "$raw" ]] && die "internal: empty backup SSH target"
  if [[ "$raw" != *@* ]]; then
    printf 'backr@%s' "$raw"
  else
    printf '%s' "$raw"
  fi
}

#
# Inputs: BACKUP_SSH_TARGET or BACKR_BACKUP_HOST when set; ~/.ssh/id_ed25519.pub must exist for bootstrap text.
# Outputs: verifies ssh BatchMode to target; if unreachable or still denied, prints pubkey + BACKR_AUTHORIZED_KEYS server command (no ssh-copy-id).
# External: ssh tests pubkey authentication without prompting (BatchMode); curl URL is Backr upstream raw script.
#
verify_pubkey_ssh_or_print_bootstrap_line() {
  local raw="" target="" pub="$HOME/.ssh/id_ed25519.pub"
  local ssh_opts=( -o BatchMode=yes -o ConnectTimeout=12 )
  local ssh_hint="ssh -o BatchMode=yes -o ConnectTimeout=12"
  raw="${BACKUP_SSH_TARGET:-}"
  [[ -n "$raw" ]] || return 0

  target="$(normalize_backup_ssh_target "$raw")"

  if [[ "${SURVEY_CLIENT_SSH_PORT:-}" == "custom" ]] && [[ -n "${SURVEY_CLIENT_SSH_CUSTOM_PORT:-}" ]]; then
    ssh_opts=( -p "${SURVEY_CLIENT_SSH_CUSTOM_PORT}" -o BatchMode=yes -o ConnectTimeout=12 )
    ssh_hint="ssh -p ${SURVEY_CLIENT_SSH_CUSTOM_PORT} -o BatchMode=yes -o ConnectTimeout=12"
  fi

  [[ -f "$pub" ]] || {
    echo "Note: missing ${pub} — generate a key before using passwordless SSH to ${target}."
    return 0
  }

  if ssh "${ssh_opts[@]}" "$target" "exit 0" 2>/dev/null; then
    echo "SSH pubkey authentication OK for ${target} (BatchMode probe)."
    return 0
  fi

  local key_line=""
  key_line="$(head -n1 "$pub" | tr -d '\r')"
  [[ -n "$key_line" ]] || return 0

  local raw_url="https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh"

  cat <<EOF

── Passwordless SSH not working yet for ${target} ──
This script does not use ssh-copy-id or SSH passwords. On the **backup machine** (console or existing admin SSH),
install your laptop pubkey for user $(printf '%s' "$target" | cut -d@ -f1) — paste exactly one line:

${key_line}

One-shot (from backup host as root), quoting preserved:

  sudo BACKR_AUTHORIZED_KEYS=$(printf '%q' "${key_line}") bash -c 'curl -fsSL ${raw_url} | bash'

Then re-run connectivity from here:

  ${ssh_hint} ${target} exit

EOF
}

main() {
  parse_args "$@"
  run_connecting_client_questionnaire
  require_linux

  if [[ -z "$BACKUP_SSH_TARGET" ]] && [[ -n "${BACKR_BACKUP_HOST:-}" ]]; then
    BACKUP_SSH_TARGET="$BACKR_BACKUP_HOST"
  fi

  # Default when no mode flags: build AppImage locally and install menu entry.
  SETUP_KIND="${SETUP_KIND:-build}"

  expand_projects_dir

  case "$SETUP_KIND" in
    download)
      install_appimage_from_network
      print_appimage_done
      ;;
    deps)
      install_connecting_os_packages
      ensure_nodejs
      ensure_rust_toolchain
      ensure_projects_dir
      maybe_create_ssh_key
      install_node_project_deps
      print_done
      ;;
    build)
      install_appimage_build_and_integrate
      print_appimage_done
      ;;
    *)
      die "internal: unknown SETUP_KIND=${SETUP_KIND}"
      ;;
  esac

  verify_pubkey_ssh_or_print_bootstrap_line
  emit_connecting_client_custom_next_steps
}

main "$@"
