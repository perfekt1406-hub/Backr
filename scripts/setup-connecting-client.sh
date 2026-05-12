#!/usr/bin/env bash
#
# Purpose: Prepare a Linux machine for Backr — by default installs deps, builds the desktop AppImage from this repo,
#          and registers it under ~/.local/share (launcher menu entry).
# Role: Distro-aware OS packages for Tauri (WebKitGTK, SSL, build tools), Node.js LTS,
#       Rust via rustup (respecting src-tauri/Cargo.toml rust-version), OpenSSH client + rsync, git/curl;
#       npm ci/npm install; optional projects dir + SSH key; npm run tauri:build + AppImage install unless --deps-only.
#
# Run from anywhere with sudo available when elevated installs are needed:
#   ./scripts/setup-connecting-client.sh [options]
#
# Options:
#   --projects-dir PATH            Local folder containing one subdirectory per project (default: ~/Projects).
#   --skip-keygen                  Do not offer to create ~/.ssh/id_ed25519 if missing.
#   --deps-only                    Install toolchain and npm deps only (no AppImage build / menu install); use for dev.
#   --install-appimage             Same as default (explicit): build AppImage locally and install launcher entry.
#   --install-appimage-build       Same as default (explicit).
#   --appimage-url URL             Download this AppImage and add launcher entry only (no compile).
#   -h, --help                     Show this text.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECTS_DIR="${PROJECTS_DIR:-$HOME/Projects}"
SKIP_KEYGEN=0
# Exclusive setup goal: build (default) | deps | download — see set_setup_kind().
SETUP_KIND=""
APPIMAGE_URL_OVERRIDE=""

APT_UPDATED=0

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '1,36p' "$0" | tail -n +2
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

Backups — copy pubkey to backup host (replace HOST/user):
  ssh-copy-id -i ~/.ssh/id_ed25519.pub backr@HOST

Then set in ~/.config/backr/config.toml after the wizard:
  [local]
  projects_path = "${PROJECTS_DIR}"

EOF
}

main() {
  parse_args "$@"
  require_linux

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
}

main "$@"
