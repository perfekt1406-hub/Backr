#!/usr/bin/env bash
#
# Purpose: Prepare a Linux machine for Backr — by default installs deps, builds the desktop AppImage from this repo,
#          registers it under ~/.local/share (AppImage + .desktop + hicolor icons + menu DB refresh), and optionally walks through two setup questions plus terms for SSH keys.
# Role: Distro-aware OS packages for Tauri (WebKitGTK, SSL, build tools), Node.js LTS,
#       Rust via rustup (respecting src-tauri/Cargo.toml rust-version), OpenSSH client + rsync, git/curl;
#       npm ci/npm install; optional projects dir + SSH key; npm run tauri:build + AppImage install unless --deps-only;
#       minimal questionnaire via Node @clack/prompts (scripts/backr-connecting-survey.mjs); requires Node 18+ and @clack/prompts in the repo.
#       SSH port + optional backup host; default ssh-copy-id when --backup-host BatchMode probe fails (Trust keys fallback),
#       tailored hints when /dev/tty exists.
#
# Run from anywhere with sudo available when elevated installs are needed:
#   ./scripts/setup-connecting-client.sh [options]
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
#   --deps-only                    Install toolchain and npm deps only (no AppImage build / menu install); use for dev.
#   --install-appimage             Same as default (explicit): build AppImage locally and install launcher entry.
#   --install-appimage-build       Same as default (explicit).
#   --appimage-url URL             Download this AppImage and add launcher entry only (no compile).
#   --reinstall-launcher           Re-copy AppImage menu entry + icons + desktop DB (no full build). Use ~/.local/share/backr/Backr.AppImage or the last built .AppImage in this repo.
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

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
SKIP_KEYGEN=0
# When 1, skip interactive ssh-copy-id after failed BatchMode probe (see BACKR_NO_SSH_COPY_ID).
SKIP_SSH_COPY_ID="${BACKR_NO_SSH_COPY_ID:-0}"
# Exclusive setup goal: build (default) | deps | download — see set_setup_kind().
SETUP_KIND=""
APPIMAGE_URL_OVERRIDE=""
REINSTALL_LAUNCHER=0
# Optional backup SSH target for pubkey probe / bootstrap hints (see verify_pubkey_ssh_or_print_bootstrap_line).
BACKUP_SSH_TARGET=""
BACKR_NON_INTERACTIVE="${BACKR_NON_INTERACTIVE:-0}"
SURVEY_SKIP_NO_TTY=0
SURVEY_CLIENT_NETWORK="${SURVEY_CLIENT_NETWORK:-unknown}"
SURVEY_CLIENT_SERVER_READY="${SURVEY_CLIENT_SERVER_READY:-unknown}"
SURVEY_CLIENT_SSH_PORT="${SURVEY_CLIENT_SSH_PORT:-unknown}"
SURVEY_CLIENT_SSH_CUSTOM_PORT="${SURVEY_CLIENT_SSH_CUSTOM_PORT:-}"
SURVEY_CLIENT_HOST_PLAN="${SURVEY_CLIENT_HOST_PLAN:-unknown}"
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
# Skips when BACKR_NON_INTERACTIVE or when no usable TTY (see emit_connecting_client_custom_next_steps).
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
  Finish backup-host setup first (curl … setup-backup-host.sh). With --backup-host, interactive runs offer ssh-copy-id when pubkey SSH fails; use --ssh-port / BACKR_SSH_PORT when sshd is not on 22. Otherwise use Backr → Trust keys (#/host/trust) or authorized_keys.

NXT
    return 0
  fi

  if [[ "${SURVEY_SKIP_NO_TTY:-0}" == "1" ]]; then
    cat <<'NXT'
• No usable interactive terminal — questionnaire was skipped (some CI environments, nested terminals, or SSH without a TTY).

  Open a normal terminal locally or use `ssh -t`, then run `./scripts/setup-connecting-client.sh` again. The wizard uses Node @clack/prompts (`npm install` in the repo installs it).

NXT
  fi

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
      apt_update_once
      run_privileged apt-get install -y \
        ca-certificates curl wget git gnupg \
        openssh-client rsync \
        build-essential pkg-config cmake mold \
        libwebkit2gtk-4.1-dev libssl-dev \
        libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev file
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
  command -v ssh-keygen &>/dev/null || return 0
  local priv="$HOME/.ssh/id_ed25519"
  [[ "$SKIP_KEYGEN" -eq 1 ]] && return 0
  if [[ -f "$priv" ]]; then
    echo "SSH private key already present: ${priv}"
    return 0
  fi
  mkdir -p "$HOME/.ssh"
  chmod 700 "$HOME/.ssh"

  if [[ "$AUTO_SSH_KEY" -eq 1 ]]; then
    echo "Creating Ed25519 SSH key (auto): ${priv}"
    # External: OpenSSH ssh-keygen writes keys under ~/.ssh (inputs: type, path, empty passphrase; outputs: keypair files).
    ssh-keygen -t ed25519 -f "$priv" -N "" -C "backr-$(whoami)@$(hostname -s 2>/dev/null || echo host)"
    echo "Created ${priv} and ${priv}.pub"
    return 0
  fi

  if [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]]; then
    echo "No Ed25519 key at ${priv} — skipping generation (--non-interactive). Use --auto-ssh-key or create a key before backups."
    return 0
  fi

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
Version=1.5
Type=Application
Name=Backr
GenericName=Backup client
Comment=Backr desktop backup client (rsync snapshots over SSH)
Exec=${dest} %u
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
  echo "Installed AppImage: ${dest}"
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
  App menu / grid: open Activities (GNOME), the KDE launcher, or your panel app menu — search «Backr».
  Note: store-style «App Center» catalogs (Snap/Flatpak/Shop) only list published packages; this install uses the standard Linux .desktop + icon theme so the app appears like any other user app.
  Binary:          ~/.local/share/backr/Backr.AppImage

EOF
}

print_done() {
  cat <<EOF

── Backr dev environment ready ──
  Repo:        ${REPO_ROOT}
  Projects:    ${PROJECTS_DIR}

  NOTE: --deps-only was used — the Backr dashboard (AppImage) was NOT installed.
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
# Inputs: none (uses REPO_ROOT and $HOME). Outputs: re-runs desktop integration for an existing AppImage.
# External: install_appimage_desktop_integration copies binary and refreshes FreeDesktop caches (see refresh_application_launcher_caches).
#
reinstall_backr_launcher_only() {
  local img=""
  if [[ -f "$HOME/.local/share/backr/Backr.AppImage" ]]; then
    img="$HOME/.local/share/backr/Backr.AppImage"
  else
    img="$(find "$REPO_ROOT/src-tauri/target/release/bundle" -type f -name '*.AppImage' 2>/dev/null | head -n1 || true)"
  fi
  [[ -n "$img" ]] && [[ -f "$img" ]] ||
    die "No AppImage found at ~/.local/share/backr/Backr.AppImage and none under src-tauri/target/release/bundle — run a full install/build first"
  echo "Re-installing launcher integration using: ${img}"
  install_appimage_desktop_integration "$img"
  print_appimage_done
  echo "Still missing from the menu? Log out and back in (or reboot). If you originally ran setup with sudo, the app may be under /root/.local — re-run this script without sudo."
}

main() {
  parse_args "$@"
  require_linux

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

  connecting_client_prepare_interactive_wizard

  if [[ "$AUTO_SSH_KEY" -eq 1 ]] || [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] || survey_tty_is_usable_client; then
    maybe_create_ssh_key
  fi

  run_connecting_client_questionnaire

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
