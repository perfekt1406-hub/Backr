#!/usr/bin/env bash
#
# Purpose: Prepare a Linux machine for Backr — by default installs deps, builds the desktop app (native binary) from this repo,
#          registers it under ~/.local/share (binary + .desktop + hicolor icons + menu DB refresh), and optionally walks through two setup questions plus terms for SSH keys.
# Role: Distro-aware OS packages for Tauri (WebKitGTK, SSL, build tools), Node.js LTS,
#       Rust via rustup (respecting src-tauri/Cargo.toml rust-version), OpenSSH client + rsync, git/curl;
#       npm ci/npm install; optional projects dir + SSH key; `tauri build --no-bundle` native-binary install unless --deps-only;
#       minimal questionnaire via Node @clack/prompts (scripts/backr-connecting-survey.mjs); requires Node 18+ and @clack/prompts in the repo.
#       SSH port + optional backup host; default ssh-copy-id when --backup-host BatchMode probe fails (Trust keys fallback),
#       tailored hints when /dev/tty exists.
#
# Run from a checkout, or as a curl one-liner (downloads the source, builds from source):
#   ./scripts/setup-connecting-client.sh [options]
#   curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-connecting-client.sh | bash
#
# Re-running reinstalls/updates: it rebuilds from the latest source and replaces the
# installed app, stopping any running instance first so the update takes effect.
# Run as your normal user (NOT sudo) — the script elevates per-command for package installs.
#
# Options:
#   --projects-dir PATH            Local folder containing one subdirectory per project (default: ~/Projects).
#   --skip-keygen                  Do not offer to create ~/.ssh/id_ed25519 if missing.
#   --backup-host TARGET           After setup: probe pubkey SSH to TARGET (host/IP or user@host; default user backr).
#                                  If login fails interactively, offers ssh-copy-id (you type SSH password once at the prompt).
#                                  Otherwise prints Trust keys (#/host/trust) / authorized_keys hints.
#   --ssh-port N                   Use TCP port N for --backup-host probes and ssh-copy-id (non-interactive pairing without the questionnaire).
#   --auto-ssh-key                 Create ~/.ssh/id_ed25519 when missing without prompting (use with CI or --non-interactive).
#   --yes-ssh-copy-id              When pubkey probe fails, run ssh-copy-id immediately without the Y/n confirmation (password still typed by OpenSSH).
#   --no-ssh-copy-id               After setup: skip ssh-copy-id offer (Trust-keys hints only when probe fails).
#   --deps-only                    Install toolchain and npm deps only (no app build / menu install); use for dev.
#   --install-appimage             Same as default (explicit): build the native binary locally and install launcher entry.
#   --install-appimage-build       Same as default (explicit).
#   --appimage-url URL             Download a prebuilt AppImage and add launcher entry only (no compile; needs libfuse2).
#   --reinstall-launcher           Re-copy the menu entry + icons + desktop DB (no full build), reusing the installed binary/AppImage.
#   --uninstall                    Remove the installed app (binary, launcher entry, icons). Keeps config, SSH keys, toolchain.
#   --non-interactive              Skip questionnaire and abbreviated default next-steps (CI / pipes).
#   -h, --help                     Show this text.
#
# Environment:
#   BACKR_BACKUP_HOST        Same as --backup-host (e.g. backr@192.168.1.10 or 192.168.1.10).
#   BACKR_SSH_PORT             Same as --ssh-port (digits only; used for BatchMode probe + ssh-copy-id).
#   BACKR_SETUP_PUBKEY_LINE    Optional single-line OpenSSH pubkey (non-interactive paste trust hints without prompts).
#   BACKR_NO_SSH_COPY_ID=1     Same as --no-ssh-copy-id (no ssh-copy-id prompt when BatchMode probe fails).
#   BACKR_YES_SSH_COPY_ID=1    Same as --yes-ssh-copy-id (skip confirmation before ssh-copy-id).
#   BACKR_AUTO_SSH_KEY=1       Same as --auto-ssh-key (create Ed25519 key without prompting when missing).
#   BACKR_NON_INTERACTIVE=1    Same as --non-interactive.

set -euo pipefail

# Resolve the repo root from the script location.  Tolerate `curl | bash`, where
# BASH_SOURCE[0] may be unset and there is no checkout — resolve_repo_source()
# then downloads the source and repoints REPO_ROOT.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd || true)"
# Absolute path of the scripts/ directory — used to locate template files such as
# backrd.service.template and backrd.plist.template when running from a local checkout.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"
# Set to 1 when REPO_ROOT is a downloaded temp tree (curl mode) so it is cleaned up.
SRC_IS_TEMP=0
PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
SKIP_KEYGEN=0
# When 1, skip interactive ssh-copy-id after failed BatchMode probe (see BACKR_NO_SSH_COPY_ID).
SKIP_SSH_COPY_ID="${BACKR_NO_SSH_COPY_ID:-0}"
# Exclusive setup goal: build (default) | deps | download — see set_setup_kind().
SETUP_KIND=""
APPIMAGE_URL_OVERRIDE=""
REINSTALL_LAUNCHER=0
DO_UNINSTALL=0
# Optional backup SSH target for pubkey probe / bootstrap hints (see verify_pubkey_ssh_or_print_bootstrap_line).
BACKUP_SSH_TARGET=""
BACKR_NON_INTERACTIVE="${BACKR_NON_INTERACTIVE:-0}"
SURVEY_SKIP_NO_TTY=0
SURVEY_CLIENT_NETWORK="${SURVEY_CLIENT_NETWORK:-unknown}"
SURVEY_CLIENT_SERVER_READY="${SURVEY_CLIENT_SERVER_READY:-unknown}"
SURVEY_CLIENT_SSH_PORT="${SURVEY_CLIENT_SSH_PORT:-unknown}"
SURVEY_CLIENT_SSH_CUSTOM_PORT="${SURVEY_CLIENT_SSH_CUSTOM_PORT:-}"
SURVEY_CLIENT_HOST_PLAN="${SURVEY_CLIENT_HOST_PLAN:-unknown}"
# Wizard's answer to "create an SSH key?" (yes|no|exists|empty). See maybe_create_ssh_key.
SURVEY_CLIENT_GEN_SSH_KEY="${SURVEY_CLIENT_GEN_SSH_KEY:-}"
BACKR_SETUP_PUBKEY_LINE="${BACKR_SETUP_PUBKEY_LINE:-}"

APT_UPDATED=0
# Automation helpers (see maybe_create_ssh_key / verify_pubkey_ssh_or_print_bootstrap_line).
AUTO_SSH_KEY=0
YES_SSH_COPY_ID=0
CLI_SSH_PORT=""
[[ "${BACKR_AUTO_SSH_KEY:-0}" == "1" ]] && AUTO_SSH_KEY=1
[[ "${BACKR_YES_SSH_COPY_ID:-0}" == "1" ]] && YES_SSH_COPY_ID=1

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '2,40p' "$0"
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
      --ssh-port)
        CLI_SSH_PORT="${2:-}"
        [[ -n "$CLI_SSH_PORT" ]] || die "--ssh-port needs a value"
        shift 2
        ;;
      --auto-ssh-key)
        AUTO_SSH_KEY=1
        shift
        ;;
      --yes-ssh-copy-id)
        YES_SSH_COPY_ID=1
        shift
        ;;
      --no-ssh-copy-id)
        SKIP_SSH_COPY_ID=1
        shift
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
      --reinstall-launcher)
        REINSTALL_LAUNCHER=1
        shift
        ;;
      --uninstall)
        DO_UNINSTALL=1
        shift
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
# Inputs: none. Outputs: returns 0 when this process can open /dev/tty read-write (usable questionnaire session).
# Notes: [[ -c /dev/tty ]] is insufficient on headless contexts — probe open instead (same as backup-host helper).
#
survey_tty_is_usable_client() {
  ( exec 3<>/dev/tty ) 2>/dev/null || return 1
  return 0
}

#
# Inputs: none — writes SURVEY_CLIENT_* via Node @clack/prompts into a temp env file and sources it.
# Outputs: returns 0 when the wizard completes; non-zero when Node errors or the script is missing.
# External: Node runs scripts/backr-connecting-survey.mjs (inputs: --env-file and backup hints; outputs: export lines).
#
run_connecting_client_questionnaire_clack() {
  local env_out=""
  env_out="$(mktemp)"
  [[ -f "$REPO_ROOT/scripts/backr-connecting-survey.mjs" ]] || {
    rm -f "$env_out"
    die "missing ${REPO_ROOT}/scripts/backr-connecting-survey.mjs — use a full repo checkout"
  }
  command -v node &>/dev/null || die "Node.js is required for the setup wizard — install Node 18+ (see https://nodejs.org/)"
  # External: node executes the ESM survey script (inputs: argv; outputs: env file + exit status).
  if node "$REPO_ROOT/scripts/backr-connecting-survey.mjs" \
    --env-file "$env_out" \
    --backup-target-cli="${BACKUP_SSH_TARGET:-}" \
    --backup-target-env="${BACKR_BACKUP_HOST:-}"; then
    # shellcheck disable=SC1090
    source "$env_out"
    rm -f "$env_out"
    return 0
  fi
  rm -f "$env_out"
  return 1
}

#
# Outputs: fills SURVEY_CLIENT_*; may set BACKUP_SSH_TARGET when user types host/IP.
# Skips when BACKR_NON_INTERACTIVE or when no usable TTY.
#
run_connecting_client_questionnaire() {
  [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] && return 0
  if ! survey_tty_is_usable_client; then
    SURVEY_SKIP_NO_TTY=1
    return 0
  fi

  if [[ ! -t 0 ]]; then
    exec </dev/tty 2>/dev/null || true
  fi

  export TERM="${TERM:-xterm-256color}"

  ensure_clack_prompts_pkg || die "failed to install @clack/prompts — run: cd ${REPO_ROOT} && npm install"
  run_connecting_client_questionnaire_clack || die "setup wizard failed — fix errors above or use --non-interactive"
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
# Inputs: none — uses REPO_ROOT and npm. Outputs: installs @clack/prompts under node_modules when missing (OpenClaw-style wizard).
# External: npm install resolves packages (inputs: package.json + registry; outputs: node_modules).
#
ensure_clack_prompts_pkg() {
  [[ -d "$REPO_ROOT/node_modules/@clack/prompts" ]] && return 0
  echo "Installing @clack/prompts for the setup wizard …"
  (cd "$REPO_ROOT" && npm install --no-audit --no-fund @clack/prompts) || return 1
}

#
# Inputs: none. Outputs: before interactive questionnaire, ensures Node/npm and @clack/prompts when a TTY questionnaire will run.
#
connecting_client_prepare_interactive_wizard() {
  [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] && return 0
  survey_tty_is_usable_client || return 0
  ensure_nodejs
  ensure_clack_prompts_pkg || die "failed to install @clack/prompts — run: cd ${REPO_ROOT} && npm install"
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
      # Skip apt entirely when every required package is already installed — avoids
      # a sudo prompt on machines that were previously set up (e.g. re-runs, reinstalls).
      local apt_pkgs=(
        ca-certificates curl wget git gnupg
        openssh-client rsync
        build-essential pkg-config cmake mold
        libwebkit2gtk-4.1-dev libssl-dev
        libayatana-appindicator3-dev librsvg2-dev
        libxdo-dev file
      )
      local missing=()
      for pkg in "${apt_pkgs[@]}"; do
        dpkg -l "$pkg" 2>/dev/null | grep -q "^ii" || missing+=("$pkg")
      done
      if [[ ${#missing[@]} -eq 0 ]]; then
        echo "All required packages already installed — skipping apt."
      else
        apt_update_once
        run_privileged apt-get install -y "${missing[@]}"
      fi
      ;;
    dnf)
      run_privileged dnf install -y \
        curl wget git openssl-devel mold \
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
        base-devel curl wget git openssl mold \
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
# Ensures mDNS (.local) name resolution works so backups can target the backup host by
# its avahi hostname (e.g. archlinux.local) and keep working across DHCP IP changes.
# Installs avahi + nss-mdns for the detected backend, adds mdns to the nsswitch hosts
# line, and enables the avahi daemon. Best-effort: warns instead of failing setup so a
# distro without these packages still installs (backups then fall back to the paired IP).
#
ensure_mdns_resolution() {
  local backend
  backend="$(detect_pkg_backend)"
  echo "Ensuring mDNS (.local) resolution so backups follow the host across IP changes …"
  case "$backend" in
    apt)     run_privileged apt-get install -y avahi-daemon libnss-mdns || true ;;
    dnf)     run_privileged dnf install -y avahi nss-mdns || true ;;
    yum)     run_privileged yum install -y avahi nss-mdns || true ;;
    pacman)  run_privileged pacman -S --noconfirm --needed avahi nss-mdns || true ;;
    zypper)  run_privileged zypper --non-interactive install -y avahi nss-mdns || true ;;
    apk)     run_privileged apk add --no-cache avahi avahi-tools || true ;;
    *)       echo "warning: unknown package backend for mDNS; if .local fails, back up by IP." >&2 ;;
  esac
  ensure_nsswitch_mdns
  enable_avahi_daemon
}

#
# Adds nss-mdns to /etc/nsswitch.conf's hosts line when absent so glibc (and thus ssh/
# rsync) resolves `.local` names. Debian's libnss-mdns auto-configures this, but Arch and
# others do not. Inserts `mdns4_minimal [NOTFOUND=return]` right after `files`.
#
ensure_nsswitch_mdns() {
  local f=/etc/nsswitch.conf
  [[ -f "$f" ]] || { echo "warning: ${f} missing; cannot enable mdns resolution." >&2; return 0; }
  if grep -qE '^hosts:.*mdns' "$f"; then
    echo "nsswitch.conf already resolves mdns."
    return 0
  fi
  run_privileged sed -i -E '/^hosts:/{ /mdns/! s/\bfiles\b/files mdns4_minimal [NOTFOUND=return]/ }' "$f" \
    && echo "Added mdns4_minimal to ${f} hosts line." \
    || echo "warning: could not edit ${f}; add 'mdns4_minimal [NOTFOUND=return]' to the hosts line manually." >&2
}

#
# Enables and starts the avahi daemon (systemd or OpenRC) so this machine can resolve
# and be resolved by `.local` names. Best-effort.
#
enable_avahi_daemon() {
  if command -v systemctl &>/dev/null; then
    run_privileged systemctl enable --now avahi-daemon 2>/dev/null \
      || run_privileged systemctl enable --now avahi-daemon.service 2>/dev/null \
      || echo "warning: could not enable avahi-daemon; start it manually for .local resolution." >&2
  elif command -v rc-update &>/dev/null; then
    run_privileged rc-update add avahi-daemon default 2>/dev/null || true
    run_privileged rc-service avahi-daemon start 2>/dev/null || true
  fi
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

#
# Ensures the SSH key Backr backups use: an existing ~/.ssh/id_ed25519 is reused
# as-is ("use if found"); a missing one is created ("create if not found") when the
# user opted in — via the wizard's "use a key?" answer (SURVEY_CLIENT_GEN_SSH_KEY=yes)
# or --auto-ssh-key / BACKR_AUTO_SSH_KEY.  The key is passphraseless so the
# scheduler / cron can authenticate unattended.  Declining only matters when no key
# exists (then it is skipped with a hint).  No interactive `read` — the choice comes
# from the clack wizard, which attaches /dev/tty and so works under `curl | bash`.
#
maybe_create_ssh_key() {
  command -v ssh-keygen &>/dev/null || return 0
  [[ "$SKIP_KEYGEN" -eq 1 ]] && return 0
  local priv="$HOME/.ssh/id_ed25519"
  # Use if found — never overwrite an existing key.
  if [[ -f "$priv" ]]; then
    echo "Using existing SSH key: ${priv}"
    return 0
  fi

  if [[ "$AUTO_SSH_KEY" -ne 1 ]] && [[ "${SURVEY_CLIENT_GEN_SSH_KEY:-}" != "yes" ]]; then
    echo "No Ed25519 key at ${priv} — skipping generation."
    echo "  Scheduled/cron backups need a passwordless key. Create one later with:"
    echo "    ssh-keygen -t ed25519 -f ${priv} -N \"\""
    echo "  or re-run with --auto-ssh-key."
    return 0
  fi

  mkdir -p "$HOME/.ssh"
  chmod 700 "$HOME/.ssh"
  echo "Creating Ed25519 SSH key: ${priv}"
  # External: OpenSSH ssh-keygen writes keys under ~/.ssh (empty passphrase so the
  # scheduler can authenticate unattended).
  ssh-keygen -t ed25519 -f "$priv" -N "" -C "backr-$(whoami)@$(hostname -s 2>/dev/null || echo host)"
  echo "Created ${priv} and ${priv}.pub"
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
# Copies Backr PNGs into ~/.local/share/icons/hicolor so Icon=com.backr.app resolves at common grid sizes.
# External: gtk-update-icon-cache rebuilds the hicolor theme index when the tool exists.
#
install_backr_icon_to_user_theme() {
  local repo="$REPO_ROOT/src-tauri/icons"
  [[ -d "$repo" ]] || return 0
  local pair="" src="" dest=""
  for pair in \
    "32x32.png|32x32" \
    "128x128.png|128x128" \
    "icon.png|256x256"; do
    src="${repo}/${pair%%|*}"
    dest="${pair##*|}"
    [[ -f "$src" ]] || continue
    mkdir -p "$HOME/.local/share/icons/hicolor/${dest}/apps"
    cp -f "$src" "$HOME/.local/share/icons/hicolor/${dest}/apps/com.backr.app.png"
  done
  if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" &>/dev/null || true
  fi
}

#
# Inputs: path to a .desktop file under the user's applications dir.
# Outputs: refreshes caches so GNOME/KDE/XDG pick up the new launcher (best-effort, non-fatal).
# External: update-desktop-database indexes ~/.local/share/applications; kbuildsycoca* refreshes KDE Plasma menus.
#
refresh_application_launcher_caches() {
  local apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
  [[ -d "$apps_dir" ]] || return 0
  if command -v update-desktop-database &>/dev/null; then
    update-desktop-database "$apps_dir" &>/dev/null || true
  fi
  if command -v kbuildsycoca6 &>/dev/null; then
    kbuildsycoca6 --noincremental &>/dev/null || true
  elif command -v kbuildsycoca5 &>/dev/null; then
    kbuildsycoca5 --noincremental &>/dev/null || true
  fi
  if command -v xdg-desktop-menu &>/dev/null; then
    xdg-desktop-menu forceupdate &>/dev/null || true
  fi
}

#
# Inputs: $1 absolute path to the app artifact (native binary or AppImage),
#         $2 destination filename under ~/.local/share/backr (e.g. "backr").
# Outputs: installs the artifact + ~/.local/share/applications/com.backr.app.desktop
#          + hicolor icons, and refreshes launcher caches.  Callers install any
#          runtime libs (e.g. libfuse2 for AppImages) beforehand.
#
install_backr_app() {
  local src="$1" dest_name="$2"
  [[ -f "$src" ]] || die "app artifact not found: $src"
  local dest_dir="$HOME/.local/share/backr"
  local dest="${dest_dir}/${dest_name}"
  mkdir -p "$dest_dir"
  # Stop any running instance, then copy in the new artifact — skip the copy when
  # re-pointing at the already-installed file (e.g. --reinstall-launcher).
  if [[ "$src" != "$dest" ]]; then
    stop_running_backr
    cp -f "$src" "$dest"
  fi
  chmod u+x "$dest" || true

  install_backr_icon_to_user_theme

  local desktop="$HOME/.local/share/applications/com.backr.app.desktop"
  mkdir -p "$(dirname "$desktop")"
  # WEBKIT_DISABLE_DMABUF_RENDERER=1 guards against white/blank windows on
  # Wayland (WebKitGTK DMA-BUF failures with rolling-release Mesa).  The binary
  # also self-applies this, but set it here too to match the host launcher.
  cat >"$desktop" <<EOF
[Desktop Entry]
Version=1.5
Type=Application
Name=Backr
GenericName=Backup client
Comment=Backr desktop backup client (rsync snapshots over SSH)
Exec=env WEBKIT_DISABLE_DMABUF_RENDERER=1 ${dest} %u
TryExec=${dest}
Icon=com.backr.app
Terminal=false
Categories=Utility;Archiving;Network;
Keywords=backup;rsync;snapshot;sync;Backr;ssh;
StartupNotify=true
StartupWMClass=com.backr.app
EOF
  chmod u+x "$desktop" || true
  refresh_application_launcher_caches
  echo "Installed: ${dest}"
  echo "Launcher entry: ${desktop} (open your app menu / Activities and search «Backr»)"
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
  # Downloaded AppImages need libfuse2 at runtime.
  ensure_appimage_runtime_libs
  install_backr_app "$tmp" "Backr.AppImage"
  # Explicit cleanup — a `trap ... RETURN` here would propagate up the call stack
  # and re-fire on the caller's return, crashing under `set -u` (the local is
  # then out of scope).
  rm -f "$tmp"
}

#
# Inputs: none (uses REPO_ROOT). Outputs: OS packages, Node, Rust, projects dir,
# npm deps, then a release build via `tauri build --no-bundle` (native binary —
# no AppImage packaging, so it is faster and needs no libfuse2), then installs the
# binary + launcher into ~/.local.
#
install_app_build_and_integrate() {
  install_connecting_os_packages
  ensure_nodejs
  ensure_rust_toolchain
  ensure_projects_dir
  install_node_project_deps
  echo "Building Backr (tauri build --no-bundle — native binary) …"
  # beforeBuildCommand copies the linuxdeploy gtk plugin into ~/.cache/tauri;
  # create it so the build doesn't fail on a fresh machine (curl one-liner).
  mkdir -p "$HOME/.cache/tauri"
  (cd "$REPO_ROOT" && npx tauri build --no-bundle)
  # tauri build only compiles the GUI crate; the daemon and CLI are sibling
  # workspace members, so build them explicitly (lands at target/release/).
  echo "Building backrd daemon + backr CLI (cargo build --release) …"
  (cd "$REPO_ROOT" && cargo build -p backrd -p backr-cli --release)
  # Install the GUI as "backr-app" so the daemon tray's "Open Backr" item
  # (which execs backr-app) resolves; the CLI name "backr" is left free for it.
  install_backr_app "$(find_built_native_binary_path)" "backr-app"
  # Expose the GUI on PATH under the name the daemon launches it by.
  mkdir -p "$HOME/.local/bin"
  ln -sf "$HOME/.local/share/backr/backr-app" "$HOME/.local/bin/backr-app"
  # Install the backr CLI onto PATH so users can drive the daemon from a terminal.
  install -m 755 "$REPO_ROOT/target/release/backr" "$HOME/.local/bin/backr"
  echo "Installed backr CLI: $HOME/.local/bin/backr"
  # Install the backrd daemon binary and register it as a user service so it
  # runs persistently in the background from the first login after install.
  install_backrd_daemon_service
}

#
# Finds the native binary produced by `tauri build --no-bundle`.
#
find_built_native_binary_path() {
  # The Cargo workspace emits all artifacts to the repo-root target/, not
  # src-tauri/target/ (which only applied before the daemon/GUI split).
  # The GUI binary is named backr-app (KTD-8); backr is the CLI.
  local bin="$REPO_ROOT/target/release/backr-app"
  [[ -f "$bin" ]] || die "build produced no binary at ${bin} — check the tauri build output"
  echo "$bin"
}

#
# Finds the first built *.AppImage under target/release/bundle (only used
# as a fallback by --reinstall-launcher when an AppImage was built previously).
#
find_built_appimage_path() {
  local hit=""
  hit="$(find "$REPO_ROOT/target/release/bundle" -type f -name '*.AppImage' 2>/dev/null | head -n1 || true)"
  [[ -n "$hit" ]] || die "build produced no .AppImage under target/release/bundle — check tauri bundle targets"
  echo "$hit"
}

#
# Copies the backrd daemon binary to ~/.local/bin/backrd and registers it as a
# persistent user service so it starts at every login.
#
# On Linux: installs a systemd user service unit from backrd.service.template,
#   substituting the binary path, then enables and starts the service.
#   Skipped gracefully when systemctl is not available (non-systemd Linux).
# On macOS: installs a launchd user agent plist from backrd.plist.template,
#   substituting the binary and log paths, then loads the agent.
#
# Inputs:  REPO_ROOT (workspace with Cargo artifacts), SCRIPT_DIR (scripts/ dir).
# Outputs: installs and starts backrd as a user-session daemon.
#
install_backrd_daemon_service() {
  local backrd_src="$REPO_ROOT/target/release/backrd"
  if [[ ! -f "$backrd_src" ]]; then
    echo "warning: backrd binary not found at ${backrd_src} — skipping daemon service install." >&2
    return 0
  fi

  # Install the backrd binary to ~/.local/bin so the service unit can reference
  # a stable path that does not change when the source tree is cleaned.
  local local_bin="$HOME/.local/bin"
  mkdir -p "$local_bin"
  local backrd_bin="$local_bin/backrd"
  cp -f "$backrd_src" "$backrd_bin"
  chmod u+x "$backrd_bin"
  echo "Installed backrd daemon: ${backrd_bin}"

  if [[ "$OSTYPE" == darwin* ]]; then
    # macOS — install as a launchd user agent.
    local plist_template="${SCRIPT_DIR}/backrd.plist.template"
    if [[ ! -f "$plist_template" ]]; then
      echo "warning: ${plist_template} not found — skipping launchd agent install." >&2
      return 0
    fi
    local log_dir="$HOME/Library/Logs/backr"
    mkdir -p "$log_dir"
    local agents_dir="$HOME/Library/LaunchAgents"
    mkdir -p "$agents_dir"
    local plist_dest="${agents_dir}/com.backr.daemon.plist"
    # Replace both placeholders: binary path and log directory.
    sed \
      -e "s|BACKRD_BIN_PATH|${backrd_bin}|g" \
      -e "s|BACKR_LOG_DIR|${log_dir}|g" \
      "$plist_template" > "$plist_dest"
    # Unload any previous instance before loading the updated plist.
    launchctl unload "$plist_dest" 2>/dev/null || true
    launchctl load -w "$plist_dest"
    echo "Registered backrd launchd agent: ${plist_dest}"
    return 0
  fi

  # Linux — install as a systemd user service when systemctl is available.
  if ! command -v systemctl &>/dev/null; then
    echo "note: systemctl not found — skipping backrd systemd service install." >&2
    return 0
  fi
  local service_template="${SCRIPT_DIR}/backrd.service.template"
  if [[ ! -f "$service_template" ]]; then
    echo "warning: ${service_template} not found — skipping systemd service install." >&2
    return 0
  fi
  local service_dir="$HOME/.config/systemd/user"
  mkdir -p "$service_dir"
  local service_dest="${service_dir}/backrd.service"
  # Replace the binary-path placeholder with the installed path.
  sed "s|BACKRD_BIN_PATH|${backrd_bin}|g" "$service_template" > "$service_dest"
  systemctl --user daemon-reload
  systemctl --user enable backrd.service
  systemctl --user start backrd.service
  echo "Registered and started backrd systemd user service."
}

#
# Stops and removes the backrd daemon service (systemd or launchd) and deletes
# the installed binary from ~/.local/bin/backrd.
# Called by uninstall_backr() so the daemon is cleaned up alongside the app.
#
# Inputs:  none (uses fixed install paths).
# Outputs: removes the service unit/plist and the daemon binary.
#
remove_backrd_daemon_service() {
  if [[ "$OSTYPE" == darwin* ]]; then
    local plist_dest="$HOME/Library/LaunchAgents/com.backr.daemon.plist"
    if [[ -f "$plist_dest" ]]; then
      launchctl unload "$plist_dest" 2>/dev/null || true
      rm -f "$plist_dest"
      echo "Removed backrd launchd agent."
    fi
  elif command -v systemctl &>/dev/null; then
    # Stop and disable before removing the unit file.
    systemctl --user stop backrd.service 2>/dev/null || true
    systemctl --user disable backrd.service 2>/dev/null || true
    rm -f "$HOME/.config/systemd/user/backrd.service"
    systemctl --user daemon-reload 2>/dev/null || true
    echo "Removed backrd systemd user service."
  fi
  # Remove the daemon binary regardless of service type.
  if [[ -f "$HOME/.local/bin/backrd" ]]; then
    rm -f "$HOME/.local/bin/backrd"
    echo "Removed backrd binary (${HOME}/.local/bin/backrd)."
  fi
}

#
# Prints post-install hints (native binary install under ~/.local/share/backr).
#
print_install_done() {
  cat <<EOF

✓ Backr installed. Open it from your app menu (search «Backr») — it will find your
  backup host and pair automatically. (Uninstall later: --uninstall.)

EOF
}

print_done() {
  cat <<EOF

── Backr dev environment ready ──
  Repo:        ${REPO_ROOT}
  Projects:    ${PROJECTS_DIR}

  NOTE: --deps-only was used — the Backr dashboard app was NOT installed.
  To build and install the app launcher, re-run without --deps-only (or pass --appimage-url URL).

Run the app in dev mode:
  cd ${REPO_ROOT}
  npm run tauri:dev

Release build + install dashboard:
  ./scripts/setup-connecting-client.sh   (no --deps-only)
  # or, if you already have a built AppImage:
  ./scripts/setup-connecting-client.sh --reinstall-launcher

Backups — default trust bootstrap when you passed **--backup-host**: if pubkey SSH isn't ready yet, the script offers **ssh-copy-id** (you type the SSH password **once** at the prompt — not stored here). **--yes-ssh-copy-id** skips the confirmation. **--ssh-port** / **BACKR_SSH_PORT** set the port when sshd is not on 22. If that isn't possible, use Backr on the backup machine → **Trust keys** (#/host/trust) or **authorized_keys**. Set **BACKR_NO_SSH_COPY_ID=1** or **--no-ssh-copy-id** to skip the offer. Re-run with **--backup-host HOST** to verify SSH after trust.

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
# Inputs: BACKUP_SSH_TARGET or BACKR_BACKUP_HOST when set; BACKR_SETUP_PUBKEY_LINE or ~/.ssh/id_ed25519.pub for fallback text.
# Outputs: verifies ssh BatchMode to target; when probe fails interactively, offers ssh-copy-id (default Y); then Trust-keys hints if still failing.
# External: ssh BatchMode probes pubkey auth; ssh-copy-id invokes ssh (password at prompt, not read by this script).
#
verify_pubkey_ssh_or_print_bootstrap_line() {
  local raw="" target="" pub="$HOME/.ssh/id_ed25519.pub"
  local batch_opts=( -o BatchMode=yes -o ConnectTimeout=12 )
  local ssh_hint="ssh -o BatchMode=yes -o ConnectTimeout=12"
  raw="${BACKUP_SSH_TARGET:-}"
  [[ -n "$raw" ]] || return 0

  target="$(normalize_backup_ssh_target "$raw")"

  local ssh_port_use=""
  if [[ -n "${CLI_SSH_PORT:-}" ]]; then
    if [[ "${CLI_SSH_PORT}" =~ ^[0-9]+$ ]] && [[ "${CLI_SSH_PORT}" -ge 1 ]] && [[ "${CLI_SSH_PORT}" -le 65535 ]]; then
      ssh_port_use="$CLI_SSH_PORT"
    else
      echo "warning: ignoring invalid --ssh-port / BACKR_SSH_PORT: ${CLI_SSH_PORT}" >&2
    fi
  fi
  if [[ -z "$ssh_port_use" ]] && [[ "${SURVEY_CLIENT_SSH_PORT:-}" == "custom" ]] && [[ -n "${SURVEY_CLIENT_SSH_CUSTOM_PORT:-}" ]]; then
    ssh_port_use="${SURVEY_CLIENT_SSH_CUSTOM_PORT}"
  fi
  if [[ -n "$ssh_port_use" ]]; then
    batch_opts=( -p "${ssh_port_use}" -o BatchMode=yes -o ConnectTimeout=12 )
    ssh_hint="ssh -p ${ssh_port_use} -o BatchMode=yes -o ConnectTimeout=12"
  fi

  [[ -f "$pub" ]] || {
    echo "Note: missing ${pub} — generate a key before using passwordless SSH to ${target}."
  }

  if ssh "${batch_opts[@]}" "$target" "exit 0" 2>/dev/null; then
    echo "SSH pubkey authentication OK for ${target} (BatchMode probe)."
    return 0
  fi

  # Default interactive path: ssh-copy-id (password is typed at OpenSSH's prompt, never stored in this script).
  if [[ "${BACKR_NON_INTERACTIVE:-0}" != "1" ]] &&
    [[ "${SKIP_SSH_COPY_ID:-0}" -eq 0 ]] &&
    survey_tty_is_usable_client &&
    [[ -f "$pub" ]] &&
    command -v ssh-copy-id &>/dev/null; then
    echo ""
    echo "── Trust bootstrap (default) ──"
    echo "ssh-copy-id will append ${pub} to ${target}'s authorized_keys."
    echo "OpenSSH will ask for your SSH password once — it is not passed on this script's command line."
    local ans=""
    if [[ "${YES_SSH_COPY_ID:-0}" -eq 1 ]]; then
      ans="y"
    else
      read -r -p "Run ssh-copy-id now? [Y/n] " ans </dev/tty 2>/dev/null || ans=""
    fi
    if [[ ! "${ans,,}" =~ ^(n|no)$ ]]; then
      local copy_args=( -i "$pub" -o StrictHostKeyChecking=accept-new )
      if [[ -n "$ssh_port_use" ]]; then
        copy_args+=( -p "${ssh_port_use}" )
      fi
      # External: ssh-copy-id runs ssh non-BatchMode so the operator can authenticate with a password once when permitted.
      if ssh-copy-id "${copy_args[@]}" "$target"; then
        if ssh "${batch_opts[@]}" "$target" "exit 0" 2>/dev/null; then
          echo "SSH pubkey authentication OK for ${target} after ssh-copy-id."
          return 0
        fi
        echo "ssh-copy-id reported success but BatchMode SSH still fails — try Trust keys or authorized_keys below." >&2
      else
        echo "ssh-copy-id did not succeed (wrong password, pubkey-only account, or server policy) — use Trust keys or manual authorized_keys below." >&2
      fi
    fi
  elif [[ "${SKIP_SSH_COPY_ID:-0}" -eq 1 ]] || [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]]; then
    :
  elif [[ ! -f "$pub" ]]; then
    :
  elif ! command -v ssh-copy-id &>/dev/null; then
    echo "Note: ssh-copy-id not found — install openssh-client or use Trust keys / authorized_keys below." >&2
  fi

  local key_line=""
  key_line="$(printf '%s' "${BACKR_SETUP_PUBKEY_LINE:-}" | tr -d '\r' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  if [[ -z "$key_line" ]] && [[ -f "$pub" ]]; then
    key_line="$(head -n1 "$pub" | tr -d '\r')"
  fi
  [[ -n "$key_line" ]] || {
    cat <<EOF

── Passwordless SSH not working yet for ${target} ──
Add your laptop's pubkey for user $(printf '%s' "$target" | cut -d@ -f1) on the backup machine: Backr sidebar «Trust keys» (hash #/host/trust), or edit ~/.ssh/authorized_keys there.

Then retry:

  ${ssh_hint} ${target} exit

EOF
    return 0
  }

  cat <<EOF

── Passwordless SSH not working yet for ${target} ──
Install your laptop pubkey for user $(printf '%s' "$target" | cut -d@ -f1): paste one line into Backr → Trust keys (#/host/trust), or append to that user's ~/.ssh/authorized_keys:

${key_line}

Then retry from here:

  ${ssh_hint} ${target} exit

EOF
}

#
# Inputs: none (uses REPO_ROOT and $HOME). Outputs: re-runs launcher/.desktop
# integration for an already-installed or freshly-built artifact (native binary
# preferred, AppImage as fallback) without rebuilding.
#
reinstall_backr_launcher_only() {
  local img="" name=""
  if [[ -f "$HOME/.local/share/backr/backr-app" ]]; then
    img="$HOME/.local/share/backr/backr-app"; name="backr-app"
  elif [[ -f "$HOME/.local/share/backr/backr" ]]; then
    img="$HOME/.local/share/backr/backr"; name="backr"
  elif [[ -f "$HOME/.local/share/backr/Backr.AppImage" ]]; then
    img="$HOME/.local/share/backr/Backr.AppImage"; name="Backr.AppImage"
  elif [[ -f "$REPO_ROOT/target/release/backr-app" ]]; then
    img="$REPO_ROOT/target/release/backr-app"; name="backr-app"
  else
    img="$(find "$REPO_ROOT/target/release/bundle" -type f -name '*.AppImage' 2>/dev/null | head -n1 || true)"; name="Backr.AppImage"
  fi
  [[ -n "$img" ]] && [[ -f "$img" ]] ||
    die "No installed or built Backr found (looked for ~/.local/share/backr/backr, an AppImage, or a release build) — run a full install first"
  echo "Re-installing launcher integration using: ${img}"
  install_backr_app "$img" "$name"
  print_install_done
  echo "Still missing from the menu? Log out and back in (or reboot). If you originally ran setup with sudo, the app may be under /root/.local — re-run this script without sudo."
}

#
# Removes the installed Backr app: native binary / AppImage, the launcher entry,
# and hicolor icons.  Leaves user data (config, SSH keys) and the build toolchain
# untouched.  Invoked by --uninstall.
#
uninstall_backr() {
  stop_running_backr
  local dir="$HOME/.local/share/backr"
  # "backr" is the legacy (pre-split) GUI name; "backr-app" is the current one.
  rm -f "${dir}/backr" "${dir}/backr-app" "${dir}/Backr.AppImage"
  rm -f "$HOME/.local/bin/backr-app" "$HOME/.local/bin/backr"
  rmdir "$dir" 2>/dev/null || true
  rm -f "$HOME/.local/share/applications/com.backr.app.desktop"
  rm -f "$HOME"/.local/share/icons/hicolor/*/apps/com.backr.app.png 2>/dev/null || true
  refresh_application_launcher_caches
  if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" &>/dev/null || true
  fi
  # Stop and remove the backrd daemon service (best-effort; non-fatal if absent).
  remove_backrd_daemon_service
  echo "Backr app removed (binary, launcher entry, icons)."
  echo "Left untouched: your config (~/.config/backr), SSH keys, toolchain (Node/Rust/system packages)."
  echo "To also clear pairing config: rm -rf \"\${XDG_CONFIG_HOME:-\$HOME/.config}/backr\""
}

#
# Detects an existing Backr installation under ~/.local/share/backr and removes it
# before a fresh build/download install.  Leaves config — config is handled separately
# by clear_backr_config_for_reinstall.  Called at the start of every build/download flow.
#
remove_existing_install_if_present() {
  local dir="$HOME/.local/share/backr"
  local found=0
  [[ -f "${dir}/backr" ]] && found=1
  [[ -f "${dir}/backr-app" ]] && found=1
  [[ -f "${dir}/Backr.AppImage" ]] && found=1
  [[ "$found" -eq 0 ]] && return 0

  echo "Existing Backr installation detected — removing before reinstall …"
  stop_running_backr
  rm -f "${dir}/backr" "${dir}/backr-app" "${dir}/Backr.AppImage"
  rm -f "$HOME/.local/bin/backr-app"
  rmdir "$dir" 2>/dev/null || true
  rm -f "$HOME/.local/share/applications/com.backr.app.desktop"
  rm -f "$HOME"/.local/share/icons/hicolor/*/apps/com.backr.app.png 2>/dev/null || true
  refresh_application_launcher_caches
  if command -v gtk-update-icon-cache &>/dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" &>/dev/null || true
  fi
  echo "Previous installation removed. Proceeding with fresh install …"
}

#
# Wipes ~/.config/backr so the app opens in setup/pairing mode after a reinstall.
# Always called for build and download flows — independent of whether a prior binary
# was found — so stale config never carries over from a previous install.
# SSH keys in ~/.ssh are never touched.
#
clear_backr_config_for_reinstall() {
  local cfg_dir
  cfg_dir="${XDG_CONFIG_HOME:-$HOME/.config}/backr"
  if [[ -d "$cfg_dir" ]]; then
    rm -rf "$cfg_dir"
    echo "Cleared Backr config (${cfg_dir}) — app will open in setup/pairing mode."
  fi
}

#
# Inputs: REPO_ROOT (from BASH_SOURCE). Outputs: a usable Backr source tree.
# When run from a checkout (package.json + src-tauri/Cargo.toml present) it is
# used as-is.  When piped via `curl … | bash` there is no checkout, so the
# latest source tarball is downloaded to a temp dir and REPO_ROOT is repointed
# there (SRC_IS_TEMP=1 so cleanup_temp_source removes it on exit).  This lets the
# same script run both from a clone and as a curl one-liner, building from source.
# External: curl downloads the GitHub branch tarball; tar extracts it.
#
resolve_repo_source() {
  if [[ -n "$REPO_ROOT" ]] && [[ -f "$REPO_ROOT/package.json" ]] && [[ -f "$REPO_ROOT/src-tauri/Cargo.toml" ]]; then
    return 0
  fi
  echo "No local repo checkout detected — downloading Backr source (curl one-liner mode) …"
  command -v curl &>/dev/null || die "curl is required to download the Backr source"
  command -v tar &>/dev/null || die "tar is required to extract the Backr source"
  local repo_slug="${BACKR_REPO_SLUG:-perfekt1406-hub/Backr}"
  local branch="${BACKR_REPO_BRANCH:-main}"
  local tarball_url="https://github.com/${repo_slug}/archive/refs/heads/${branch}.tar.gz"
  local tmp
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/backr-client-src.XXXXXX")"
  echo "Downloading ${tarball_url} …"
  if ! curl -fsSL "$tarball_url" | tar -xz -C "$tmp" --strip-components=1; then
    rm -rf "$tmp"
    die "failed to download Backr source from ${tarball_url} (set BACKR_REPO_SLUG / BACKR_REPO_BRANCH to override)"
  fi
  REPO_ROOT="$tmp"
  SRC_IS_TEMP=1
  echo "Using downloaded source at ${REPO_ROOT}"
}

#
# Removes the temp source tree downloaded by resolve_repo_source (curl mode).
# No-op for local checkouts.  Registered on EXIT.
#
cleanup_temp_source() {
  [[ "${SRC_IS_TEMP:-0}" -eq 1 ]] || return 0
  [[ -n "${REPO_ROOT:-}" && -d "$REPO_ROOT" ]] && rm -rf "$REPO_ROOT"
  return 0
}

#
# Stops any running Backr instance before a (re)install.  A running AppImage
# holds its file open, so overwriting it would fail with "text file busy", and
# tauri-plugin-single-instance would otherwise keep focusing the old version
# after an update.  Best-effort — absent pkill or no match is fine.
#
stop_running_backr() {
  command -v pkill &>/dev/null || return 0
  # "backr-app" is the current GUI process name; "backr" matches legacy installs.
  if pkill -x backr-app 2>/dev/null || pkill -x backr 2>/dev/null \
     || pkill -f '/Backr\.AppImage' 2>/dev/null; then
    echo "Stopped a running Backr instance so it can be updated."
    sleep 1
  fi
  return 0
}

#
# Installs the libfuse2 runtime that AppImages need to launch (best-effort, per
# distro).  Without it the freshly built AppImage fails to start on a clean
# machine (e.g. Debian 12+, which no longer ships libfuse2 by default).  Skipped
# when we cannot elevate.  WebKitGTK/other runtime libs come in as deps of the
# -dev packages from install_connecting_os_packages.
#
ensure_appimage_runtime_libs() {
  [[ "${EUID:-0}" -eq 0 ]] || command -v sudo &>/dev/null || return 0
  local backend
  backend="$(detect_pkg_backend)"
  echo "Ensuring AppImage runtime (libfuse2) for ${backend} …"
  case "$backend" in
    apt)
      apt_update_once
      run_privileged apt-get install -y libfuse2 2>/dev/null ||
        run_privileged apt-get install -y libfuse2t64 2>/dev/null || true
      ;;
    dnf) run_privileged dnf install -y fuse-libs 2>/dev/null || true ;;
    yum) run_privileged yum install -y fuse-libs 2>/dev/null || true ;;
    pacman) run_privileged pacman -Sy --noconfirm fuse2 2>/dev/null || true ;;
    zypper) run_privileged zypper --non-interactive install -y libfuse2 2>/dev/null || true ;;
    apk) run_privileged apk add --no-cache fuse 2>/dev/null || true ;;
  esac
}

#
# Ensures this machine opens Backr in CLIENT mode.  The app boots the host
# dashboard whenever /etc/backr/host.toml exists (written by setup-backup-host.sh),
# so a box that was ever a backup host would otherwise show the host UI even after
# a client install.  Setting up a client means "make this a client", so remove the
# marker (best-effort, needs root/sudo).  Re-run setup-backup-host.sh to flip back.
#
clear_host_marker_for_client() {
  local marker="/etc/backr/host.toml"
  if [[ -f "$marker" ]]; then
    echo "Found host-dashboard marker ${marker} — this machine was set up as a Backr HOST."
    echo "Removing it so Backr opens in CLIENT mode here (re-run setup-backup-host.sh to make it a host again)."
    if [[ "${EUID:-0}" -eq 0 ]] || command -v sudo &>/dev/null; then
      run_privileged rm -f "$marker" 2>/dev/null ||
        echo "warning: could not remove ${marker} — remove it manually: sudo rm -f ${marker}" >&2
    else
      echo "warning: need root/sudo to remove ${marker} — run: sudo rm -f ${marker}" >&2
    fi
  fi
  echo "Backr will open in CLIENT mode on this machine."
}

main() {
  parse_args "$@"
  require_linux

  if [[ "${DO_UNINSTALL:-0}" -eq 1 ]]; then
    uninstall_backr
    exit 0
  fi

  if [[ "${REINSTALL_LAUNCHER:-0}" -eq 1 ]]; then
    [[ -z "${SETUP_KIND:-}" ]] || die "use --reinstall-launcher without --deps-only or --appimage-url"
    reinstall_backr_launcher_only
    exit 0
  fi

  if [[ -z "${CLI_SSH_PORT:-}" ]] && [[ -n "${BACKR_SSH_PORT:-}" ]]; then
    CLI_SSH_PORT="$BACKR_SSH_PORT"
  fi
  if [[ -n "${CLI_SSH_PORT:-}" ]]; then
    [[ "${CLI_SSH_PORT}" =~ ^[0-9]+$ ]] || die "--ssh-port / BACKR_SSH_PORT must be digits only, got: ${CLI_SSH_PORT}"
    [[ "${CLI_SSH_PORT}" -ge 1 && "${CLI_SSH_PORT}" -le 65535 ]] || die "--ssh-port / BACKR_SSH_PORT out of range: ${CLI_SSH_PORT}"
  fi

  # Ensure a source tree exists (download it in curl one-liner mode) and clean
  # up any temp download on exit, before anything that reads from REPO_ROOT.
  resolve_repo_source
  trap cleanup_temp_source EXIT

  connecting_client_prepare_interactive_wizard

  run_connecting_client_questionnaire

  if [[ -z "$BACKUP_SSH_TARGET" ]] && [[ -n "${BACKR_BACKUP_HOST:-}" ]]; then
    BACKUP_SSH_TARGET="$BACKR_BACKUP_HOST"
  fi

  # Create the SSH key per the wizard's answer (or --auto-ssh-key), after the
  # questionnaire so its choice is available and before the steps that use the key.
  maybe_create_ssh_key

  # Default when no mode flags: build the native binary locally and install menu entry.
  SETUP_KIND="${SETUP_KIND:-build}"

  expand_projects_dir

  case "$SETUP_KIND" in
    download)
      remove_existing_install_if_present
      clear_backr_config_for_reinstall
      clear_host_marker_for_client
      install_appimage_from_network
      ensure_mdns_resolution
      print_install_done
      ;;
    deps)
      install_connecting_os_packages
      ensure_mdns_resolution
      ensure_nodejs
      ensure_rust_toolchain
      ensure_projects_dir
      install_node_project_deps
      print_done
      ;;
    build)
      remove_existing_install_if_present
      clear_backr_config_for_reinstall
      clear_host_marker_for_client
      install_app_build_and_integrate
      ensure_mdns_resolution
      print_install_done
      ;;
    *)
      die "internal: unknown SETUP_KIND=${SETUP_KIND}"
      ;;
  esac

  verify_pubkey_ssh_or_print_bootstrap_line
}

main "$@"
