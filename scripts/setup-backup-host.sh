#!/usr/bin/env bash
#
# Purpose: Turn a generic Linux machine into a Backr backup target with minimal operator effort — packages, sshd,
#          firewall rules when a manager is already active, authorized_keys scaffold, backup tree, host-dashboard marker,
#          Backr AppImage download + host-mode launcher (default), auto-launch of the app after setup, and optional
#          interactive questionnaire + tailored next-steps.
# Role: Distro-aware install of OpenSSH server + rsync; validates sshd_config; ensures drop-in snippets load;
#       Pubkeys normally via Backr Trust (`#/host/trust`), SSH, or console; optional BACKR_TRUST_PUBKEY / --trust-pubkey-file appends automatically. Match blocks disable SSH passwords for
#       BACKR_USER once authorized_keys holds keys; UFW/firewalld SSH allowance when already enforcing; SELinux
#       ~/.ssh contexts when enforcing; prints auto-detected OS/firewall/sshd facts (sshd -T, ss listeners);
#       downloads Backr AppImage by default (BACKR_DEFAULT_APPIMAGE_URL / --appimage-url) and installs a host-mode
#       .desktop launcher (desktop user auto-detected from SUDO_USER or overridden with --desktop-user); after install,
#       auto-launches the app so it opens immediately showing the host dashboard; use --no-appimage to skip on headless hosts;
#       optional questionnaire uses Node @clack/prompts (installs Node 18+ + npm deps in /tmp when needed) + tailored next-steps.
#
# Typical usage (on the backup machine, one command):
#   curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash
#
# The Backr host-dashboard app is downloaded and installed by default. Use --no-appimage on headless servers.
#
# Trust laptops via Backr → Trust keys (#/host/trust) once the app opens, or into ~BACKR_USER/.ssh/authorized_keys.
#
# Run with sudo on the machine that will receive rsync over SSH. Non-Linux hosts are not supported here.
#
# CLI options:
#   --user NAME               Dedicated account (default: backr).
#   --root PATH               Absolute backup root on disk (default: /srv/backr).
#   --no-firewall             Do not add SSH allowances to UFW or firewalld even when active.
#   --non-interactive         Skip the questionnaire and abbreviated default next-steps (for pipes / CI).
#   --trust-pubkey-file PATH  Append OpenSSH pubkey lines from this file to BACKR_USER's authorized_keys when missing.
#   --appimage-url URL        Download this URL instead of the default AppImage release.
#   --no-appimage             Skip the Backr AppImage download and auto-launch (headless/server installs).
#   --desktop-user USER       OS user who gets the Backr AppImage + .desktop entry (default: $SUDO_USER or first non-system user).
#   --dry-run                 Print actions only.
#   --verbose                 Print detected OS/firewall/sshd diagnostics after setup.
#   -h, --help                Show this text.
#
# Environment:
#   BACKR_NON_INTERACTIVE=1       Same as --non-interactive.
#   BACKR_TRUST_PUBKEY            One-line OpenSSH public key to append if not already present.
#   BACKR_TRUST_PUBKEY_FILE       Same as --trust-pubkey-file when the CLI flag is not passed.
#   BACKR_SCRIPTS_RAW_BASE        Base URL for raw scripts when this file is piped from curl (default: GitHub main scripts/).
#   BACKR_HOST_APPIMAGE_URL       Override download URL (same as --appimage-url).
#   BACKR_DEFAULT_APPIMAGE_URL    Override the built-in default release URL without pinning a specific build.
#   BACKR_NO_HOST_APPIMAGE=1      Same as --no-appimage.
#   BACKR_HOST_DESKTOP_USER       Same as --desktop-user.

set -euo pipefail

BACKR_USER="${BACKR_USER:-backr}"
BACKR_ROOT="${BACKR_ROOT:-/srv/backr}"
DRY_RUN=0
VERBOSE=0
SKIP_FIREWALL=0
# Optional pubkey bootstrap (see append_trust_pubkeys_from_cli_or_env).
TRUST_PUBKEY_FILE_CLI=""
# Questionnaire / non-interactive (see run_backup_host_questionnaire).
BACKR_NON_INTERACTIVE="${BACKR_NON_INTERACTIVE:-0}"
SURVEY_SKIP_NO_TTY=0
SURVEY_DEPLOYMENT="${SURVEY_DEPLOYMENT:-unknown}"
SURVEY_REACH="${SURVEY_REACH:-unknown}"
SURVEY_SSH_PORT="${SURVEY_SSH_PORT:-unknown}"
SURVEY_SSH_CUSTOM_PORT="${SURVEY_SSH_CUSTOM_PORT:-}"
SURVEY_PLATFORM="${SURVEY_PLATFORM:-unknown}"
SURVEY_KEYPATH="${SURVEY_KEYPATH:-unknown}"
BACKR_SCRIPTS_RAW_BASE="${BACKR_SCRIPTS_RAW_BASE:-https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts}"
# Default AppImage download URL used when --no-appimage is not passed.
# Override with --appimage-url or BACKR_HOST_APPIMAGE_URL to use a different build.
BACKR_DEFAULT_APPIMAGE_URL="${BACKR_DEFAULT_APPIMAGE_URL:-https://github.com/perfekt1406-hub/Backr/releases/latest/download/Backr.AppImage}"
# Host-dashboard AppImage install (see install_host_app_from_appimage_url / detect_desktop_user).
# Resolved in main() — override URL via --appimage-url / BACKR_HOST_APPIMAGE_URL, or skip with --no-appimage / BACKR_NO_HOST_APPIMAGE=1.
HOST_APPIMAGE_URL="${BACKR_HOST_APPIMAGE_URL:-}"
HOST_DESKTOP_USER="${BACKR_HOST_DESKTOP_USER:-}"
SKIP_HOST_APPIMAGE="${BACKR_NO_HOST_APPIMAGE:-0}"

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '2,32p' "$0"
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
      --no-firewall)
        SKIP_FIREWALL=1
        shift
        ;;
      --non-interactive)
        BACKR_NON_INTERACTIVE=1
        shift
        ;;
      --dry-run)
        DRY_RUN=1
        shift
        ;;
      --verbose)
        VERBOSE=1
        shift
        ;;
      --trust-pubkey-file)
        TRUST_PUBKEY_FILE_CLI="${2:-}"
        [[ -n "$TRUST_PUBKEY_FILE_CLI" ]] || die "--trust-pubkey-file needs a path"
        shift 2
        ;;
      --appimage-url)
        HOST_APPIMAGE_URL="${2:-}"
        [[ -n "$HOST_APPIMAGE_URL" ]] || die "--appimage-url needs a URL value"
        shift 2
        ;;
      --desktop-user)
        HOST_DESKTOP_USER="${2:-}"
        [[ -n "$HOST_DESKTOP_USER" ]] || die "--desktop-user needs a username value"
        shift 2
        ;;
      --no-appimage)
        SKIP_HOST_APPIMAGE=1
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
# Notes: [[ -c /dev/tty ]] matches character devices that are still unusable on headless/cloud contexts — probe open instead.
#
survey_tty_is_usable() {
  ( exec 3<>/dev/tty ) 2>/dev/null || return 1
  return 0
}

#
# Inputs: none — must run as root. Outputs: ensures Node.js 18+ and npm for @clack/prompts (same distros as install_server_ssh_rsync).
# External: apt-get/curl NodeSource setup_22.x, dnf/yum/pacman/zypper/apk install nodejs + npm where applicable.
#
ensure_nodejs_for_host_survey() {
  if command -v node &>/dev/null && command -v npm &>/dev/null; then
    local major
    major="$(node -p 'parseInt(process.versions.node,10)' 2>/dev/null || echo 0)"
    if [[ "${major:-0}" -ge 18 ]]; then
      echo "Node.js OK for setup wizard: $(node --version)"
      return 0
    fi
    echo "Node.js too old for @clack/prompts — upgrading …" >&2
  fi

  local backend
  backend="$(detect_pkg_backend)"
  echo "Installing Node.js 18+ for the Backr setup wizard (backend: ${backend})…" >&2
  export DEBIAN_FRONTEND=noninteractive

  case "$backend" in
    apt)
      apt-get update -qq
      apt-get install -y ca-certificates curl gnupg
      curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
      apt-get update -qq
      apt-get install -y nodejs
      ;;
    dnf)
      dnf install -y nodejs npm
      ;;
    yum)
      yum install -y nodejs npm || die "yum nodejs missing — install Node.js 18+ manually (see https://nodejs.org/)"
      ;;
    pacman)
      pacman -Sy --noconfirm nodejs npm
      ;;
    zypper)
      zypper --non-interactive refresh
      zypper --non-interactive install -y nodejs22 npm22 2>/dev/null ||
        zypper --non-interactive install -y nodejs npm
      ;;
    apk)
      apk update
      apk add --no-cache nodejs npm
      ;;
    *)
      die "unsupported distro for automatic Node install — install Node.js 18+ manually, then re-run this script"
      ;;
  esac

  command -v node &>/dev/null || die "node not found after install"
  command -v npm &>/dev/null || die "npm not found after install"
  major="$(node -p 'parseInt(process.versions.node,10)' 2>/dev/null || echo 0)"
  [[ "${major:-0}" -ge 18 ]] || die "Node.js 18+ required for setup wizard (got $(node --version 2>/dev/null || echo none))"
  echo "Node.js OK: $(node --version) / npm $(npm --version)"
}

#
# Runs an interactive @clack/prompts questionnaire when a usable TTY exists and BACKR_NON_INTERACTIVE is unset.
# Outputs: fills SURVEY_* globals from the questionnaire (sources a temp env file from Node).
# External: node runs scripts/backr-host-survey.mjs; npm installs @clack/prompts in a temp dir; curl may fetch the mjs from BACKR_SCRIPTS_RAW_BASE.
#
run_backup_host_questionnaire() {
  [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] && return 0
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  if ! survey_tty_is_usable; then
    SURVEY_SKIP_NO_TTY=1
    return 0
  fi

  # curl … | sudo bash leaves stdin on a pipe — attach stdin to the real terminal
  # so Clack reads behave consistently.  Save the original fd so we can restore
  # it after the questionnaire — leaving stdin on /dev/tty causes backgrounded
  # processes to hold the tty open and prevents the script from exiting.
  local need_stdin_restore=0
  if [[ ! -t 0 ]]; then
    exec 3<&0          # save original stdin to fd 3
    exec </dev/tty 2>/dev/null || true
    need_stdin_restore=1
  fi

  export TERM="${TERM:-xterm-256color}"

  ensure_nodejs_for_host_survey

  local work="" mjs="" env_out="" survey_src="" base=""
  work="$(mktemp -d "${TMPDIR:-/tmp}/backr-host-survey.XXXXXX")"

  mjs="${work}/backr-host-survey.mjs"
  survey_src=""
  if [[ -n "${BASH_SOURCE[0]:-}" ]] && [[ -f "${BASH_SOURCE[0]}" ]] && [[ "${BASH_SOURCE[0]}" != /dev/* ]]; then
    survey_src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/backr-host-survey.mjs"
  fi
  if [[ -n "$survey_src" ]] && [[ -f "$survey_src" ]]; then
    cp -f "$survey_src" "$mjs"
  else
    command -v curl &>/dev/null || {
      rm -rf "$work"
      die "curl required to fetch setup wizard (install curl or run from a git clone with scripts/)"
    }
    base="${BACKR_SCRIPTS_RAW_BASE:-https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts}"
    curl -fsSL "${base}/backr-host-survey.mjs" -o "$mjs" || {
      rm -rf "$work"
      die "failed to download backr-host-survey.mjs from ${base} (set BACKR_SCRIPTS_RAW_BASE if needed)"
    }
  fi

  if ! (cd "$work" && npm init -y >/dev/null 2>&1 && npm install --no-audit --no-fund '@clack/prompts@^1.3.0' >/dev/null); then
    rm -rf "$work"
    die "failed to install @clack/prompts for setup wizard (check npm / network)"
  fi

  env_out="$(mktemp)"
  if ! (cd "$work" && node backr-host-survey.mjs --env-file "$env_out" --backr-user="${BACKR_USER}"); then
    rm -f "$env_out"
    rm -rf "$work"
    die "setup wizard failed or was interrupted"
  fi

  # shellcheck disable=SC1090
  source "$env_out"
  rm -f "$env_out"
  rm -rf "$work"

  # Restore stdin so later backgrounded processes don't inherit /dev/tty.
  if [[ "$need_stdin_restore" -eq 1 ]]; then
    exec <&3 3<&-      # restore original stdin, close fd 3
  fi
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
# Inputs: one pubkey line. Outputs: true if line looks like an OpenSSH authorized_keys entry (ssh-* prefix).
#
is_ssh_pubkey_line() {
  [[ "$1" =~ ^(ssh-rsa|ssh-ed25519|ssh-dss|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521|sk-ssh-ed25519|sk-ecdsa-sha2-nistp256) ]]
}

#
# Inputs: path to authorized_keys. Outputs: number of plausible pubkey lines (printed to stdout).
#
count_pubkey_lines_in_authorized_keys() {
  local ak="$1" n=0 line=""
  [[ -f "$ak" ]] || {
    echo 0
    return 0
  }
  while IFS= read -r line || [[ -n "${line:-}" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" ]] || [[ "$line" =~ ^# ]] && continue
    if is_ssh_pubkey_line "$line"; then
      n=$((n + 1))
    fi
  done <"$ak"
  echo "$n"
}

#
# Inputs: none. Outputs: ensures main sshd_config includes /etc/ssh/sshd_config.d/*.conf when missing (mutates config once).
# External: sshd -t validates config before returning success.
#
ensure_sshd_includes_drop_in_dir() {
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  local main="/etc/ssh/sshd_config"
  [[ -f "$main" ]] || return 0
  if grep -qE '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\.d/\*\.conf' "$main"; then
    return 0
  fi
  if grep -qE '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\.d/' "$main"; then
    return 0
  fi

  echo "Adding Include /etc/ssh/sshd_config.d/*.conf to ${main} so drop-ins apply …"
  local bak="${main}.bak-backr-$$"
  # External: cp preserves sshd_config before append (inputs: path; outputs: backup file).
  cp -a "$main" "$bak"
  {
    printf '\n# Added by Backr setup-backup-host.sh — load drop-in snippets\n'
    printf 'Include /etc/ssh/sshd_config.d/*.conf\n'
  } >>"$main"
  if ! sshd -t 2>/dev/null; then
    cp -a "$bak" "$main"
    die "sshd -t failed after Include append — restored ${main} from backup"
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

  # External: systemctl manages SSH units on systemd desktops/servers (inputs: unit names; outputs: enabled service).
  if command -v systemctl &>/dev/null; then
    if systemctl cat ssh.service &>/dev/null 2>&1; then
      systemctl enable --now ssh 2>/dev/null || true
    fi
    if systemctl cat sshd.service &>/dev/null 2>&1; then
      systemctl enable --now sshd 2>/dev/null || true
    fi
  fi

  # External: OpenRC on Alpine-style hosts.
  if command -v rc-update &>/dev/null; then
    ssh-keygen -A 2>/dev/null || true
    rc-update add sshd default 2>/dev/null || true
    rc-service sshd start 2>/dev/null || true
  elif ! systemctl is-active --quiet ssh 2>/dev/null && ! systemctl is-active --quiet sshd 2>/dev/null; then
    echo "warning: SSH service not reported active — verify sshd is listening (systemctl status ssh or sshd)" >&2
  fi
}

#
# Inputs: $1 dry-run flag (1 = print only). Writes drop-in after authorized_keys is final for this run.
# Outputs: PubkeyAuthentication yes; when ≥1 pubkey line exists for BACKR_USER, Match User disables password/KbdInteractive for that account only.
#
sshd_write_backr_drop_in() {
  local dry="${1:-0}"
  local drop_in="/etc/ssh/sshd_config.d/99-backr.conf"
  local home="" ak="" n_keys=0

  if [[ "$dry" -eq 1 ]]; then
    echo "[dry-run] write ${drop_in} (PubkeyAuthentication; Match User password off when keys present)"
    echo "[dry-run] sshd -t && systemctl reload ssh/sshd"
    return 0
  fi

  home="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  [[ -n "$home" ]] || die "could not resolve home for ${BACKR_USER}"
  ak="${home}/.ssh/authorized_keys"
  n_keys="$(count_pubkey_lines_in_authorized_keys "$ak")"

  mkdir -p /etc/ssh/sshd_config.d
  {
    echo '# Managed by Backr setup-backup-host.sh — backup-role SSH policy.'
    echo 'PubkeyAuthentication yes'
    if [[ "$n_keys" -ge 1 ]]; then
      echo ''
      echo "Match User ${BACKR_USER}"
      echo '    PasswordAuthentication no'
      echo '    KbdInteractiveAuthentication no'
    fi
  } >"$drop_in"
  chmod 644 "$drop_in"

  sshd -t || die "sshd -t failed — fix sshd_config before continuing"

  if systemctl cat ssh.service &>/dev/null 2>&1; then
    systemctl reload ssh 2>/dev/null || systemctl restart ssh 2>/dev/null || true
  fi
  if systemctl cat sshd.service &>/dev/null 2>&1; then
    systemctl reload sshd 2>/dev/null || systemctl restart sshd 2>/dev/null || true
  fi
  if command -v rc-service &>/dev/null; then
    rc-service sshd reload 2>/dev/null || rc-service sshd restart 2>/dev/null || true
  fi

  sshd -t || die "sshd -t failed after sshd reload"
  if [[ "$n_keys" -ge 1 ]]; then
    echo "SSH for ${BACKR_USER}: pubkey required (password/KbdInteractive disabled via Match User)."
  fi
}

#
# Inputs: SKIP_FIREWALL global. Outputs: when UFW or firewalld is already active, ensures SSH is permitted (never enables a firewall).
# External: ufw / firewall-cmd mutate live firewall rules when those stacks are running.
#
open_ssh_on_active_managed_firewalls() {
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  [[ "$SKIP_FIREWALL" -eq 1 ]] && echo "Skipping firewall tweaks (--no-firewall)." && return 0

  if command -v ufw &>/dev/null; then
    if ufw status 2>/dev/null | grep -qiE 'Status:[[:space:]]*active'; then
      echo "UFW is active — ensuring inbound SSH is allowed …"
      if ! ufw status 2>/dev/null | grep -qiE '(OpenSSH|22/tcp|SSH\))'; then
        ufw allow OpenSSH 2>/dev/null || ufw allow 22/tcp comment 'backr-setup' 2>/dev/null || true
      fi
    fi
  fi

  # External: firewalld exposes persistent service rules (inputs: service names; outputs: runtime+permanent config).
  if command -v firewall-cmd &>/dev/null && systemctl is-active --quiet firewalld 2>/dev/null; then
    echo "firewalld is active — ensuring ssh service is allowed …"
    firewall-cmd --permanent --query-service=ssh &>/dev/null ||
      firewall-cmd --permanent --add-service=ssh 2>/dev/null || true
    firewall-cmd --reload 2>/dev/null || true
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

  # The Backr host app runs as the desktop user, which cannot write this
  # backup-user-owned authorized_keys directly.  Install a privileged helper +
  # passwordless-sudo rule so one-tap pairing can trust laptop keys with no manual
  # step (replaces the old POSIX-ACL approach, which depended on filesystem ACL
  # support and exact user/path matching and proved unreliable).
  install_trust_helper
}

#
# Inputs: globals BACKR_USER; uses detect_desktop_user for the sudoers principal.
# Outputs: installs /usr/local/lib/backr/append-trusted-key (root:root 0755) and a
#          NOPASSWD sudoers drop-in so the desktop user can append client pubkeys to
#          ~BACKR_USER/.ssh/authorized_keys as root during pairing.  No-op in dry-run.
# External: visudo -cf validates the drop-in before it is trusted (inputs: file; outputs: exit status).
#
install_trust_helper() {
  local helper_dir="/usr/local/lib/backr"
  local helper="${helper_dir}/append-trusted-key"
  local sudoers="/etc/sudoers.d/10-backr-trust"
  local du
  du="$(detect_desktop_user 2>/dev/null)" || {
    echo "warning: no desktop user detected — pairing will fall back to the manual sudo snippet." >&2
    return 0
  }

  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] install ${helper} + ${sudoers} (NOPASSWD for ${du})"
    return 0
  fi

  mkdir -p "$helper_dir"
  # Quoted heredoc: nothing expands at install time — the helper resolves the
  # backup user (from /etc/backr/host.toml) and home dir at run time on the host.
  cat >"$helper" <<'HELPER_EOF'
#!/usr/bin/env bash
#
# Backr privileged trust helper.  Reads one OpenSSH public key line from stdin and
# appends it to the backup user's authorized_keys as root.  Invoked by the Backr
# host app via passwordless `sudo -n` so one-tap pairing trusts laptop keys without
# an interactive prompt.  Installed by setup-backup-host.sh.
#
set -euo pipefail

marker="/etc/backr/host.toml"
ssh_user="backr"
if [[ -r "$marker" ]]; then
  v="$(sed -n 's/^[[:space:]]*ssh_user[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' "$marker" | head -n1)"
  [[ -n "$v" ]] && ssh_user="$v"
fi

home="$(getent passwd "$ssh_user" | cut -d: -f6)"
[[ -n "$home" ]] || { echo "no home for ${ssh_user}" >&2; exit 2; }

# First non-empty, whitespace-trimmed line from stdin is the candidate key.
key=""
while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  [[ -n "$line" ]] && { key="$line"; break; }
done
[[ -n "$key" ]] || { echo "empty key" >&2; exit 3; }

case "$key" in
  ssh-rsa\ *|ssh-ed25519\ *|ssh-dss\ *|\
  ecdsa-sha2-nistp256\ *|ecdsa-sha2-nistp384\ *|ecdsa-sha2-nistp521\ *|\
  sk-ssh-ed25519@openssh.com\ *|sk-ecdsa-sha2-nistp256@openssh.com\ *) ;;
  *) echo "not a valid OpenSSH public key line" >&2; exit 4 ;;
esac

ak="${home}/.ssh/authorized_keys"
install -d -m 700 -o "$ssh_user" -g "$ssh_user" "${home}/.ssh"
touch "$ak"
if grep -Fxq -- "$key" "$ak" 2>/dev/null; then
  chown "${ssh_user}:${ssh_user}" "$ak"; chmod 600 "$ak"
  echo "already present"; exit 0
fi
printf '%s\n' "$key" >>"$ak"
chown "${ssh_user}:${ssh_user}" "$ak"
chmod 600 "$ak"
echo "appended"
HELPER_EOF
  chmod 755 "$helper"
  chown root:root "$helper"

  # NOPASSWD sudoers rule scoped to exactly this helper.  Validate before trusting:
  # an invalid drop-in can lock sudo out, so write to a temp file, syntax-check with
  # visudo, and only then move it into place with the required 0440 perms.
  local tmp
  tmp="$(mktemp)"
  printf '%s ALL=(root) NOPASSWD: %s\n' "$du" "$helper" >"$tmp"
  if visudo -cf "$tmp" &>/dev/null; then
    install -m 0440 -o root -g root "$tmp" "$sudoers"
    echo "Installed trust helper + NOPASSWD rule for ${du} (one-tap pairing writes keys as root)."
  else
    echo "warning: generated sudoers rule failed validation — pairing will fall back to the manual sudo snippet." >&2
  fi
  rm -f "$tmp"
}

#
# Inputs: $1 absolute authorized_keys path; $2 one candidate line; $3 BACKR_USER for chown.
# Outputs: appends the line when it is a valid OpenSSH pubkey and not already present; otherwise no-op or warning.
# External: grep -Fxq tests exact-line membership (inputs: pattern line + file; outputs: exit status).
#
append_one_trust_pubkey_line_if_new() {
  local ak="$1" candidate="$2" owner="$3"
  candidate="${candidate#"${candidate%%[![:space:]]*}"}"
  candidate="${candidate%"${candidate##*[![:space:]]}"}"
  [[ -n "$candidate" ]] || return 0
  [[ "$candidate" =~ ^# ]] && return 0
  if ! is_ssh_pubkey_line "$candidate"; then
    echo "warning: skipping non-pubkey line in trust pubkey input (expected ssh-ed25519, ssh-rsa, …)" >&2
    return 0
  fi
  if grep -Fxq -- "$candidate" "$ak" 2>/dev/null; then
    echo "Trust pubkey line already in ${ak} — skipping duplicate."
    return 0
  fi
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] append one trust pubkey line to ${ak}"
    return 0
  fi
  printf '%s\n' "$candidate" >>"$ak"
  run_cmd chown "${owner}:${owner}" "$ak"
  run_cmd chmod 600 "$ak"
  echo "Appended trust pubkey to ${ak}."
}

#
# Inputs: globals TRUST_PUBKEY_FILE_CLI, BACKR_TRUST_PUBKEY_FILE, BACKR_TRUST_PUBKEY, BACKR_USER, DRY_RUN.
# Outputs: merges new pubkey lines into ~BACKR_USER/.ssh/authorized_keys when provided (CLI file wins over env file path).
# External: read reads file lines (inputs: fd; outputs: line variable).
#
append_trust_pubkeys_from_cli_or_env() {
  local home_dir="" ak="" eff_file="" line=""
  home_dir="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  [[ -n "$home_dir" ]] || die "could not resolve home for ${BACKR_USER}"
  ak="${home_dir}/.ssh/authorized_keys"
  [[ -f "$ak" ]] || die "internal: authorized_keys missing — ensure_ssh_dir must run first"

  eff_file="${TRUST_PUBKEY_FILE_CLI:-${BACKR_TRUST_PUBKEY_FILE:-}}"
  if [[ -n "$eff_file" ]]; then
    [[ -f "$eff_file" ]] || die "trust pubkey file not found: ${eff_file}"
    [[ -r "$eff_file" ]] || die "trust pubkey file not readable: ${eff_file}"
    echo "Reading trust pubkeys from: ${eff_file}"
    while IFS= read -r line || [[ -n "${line:-}" ]]; do
      append_one_trust_pubkey_line_if_new "$ak" "$line" "$BACKR_USER"
    done <"$eff_file"
  fi

  if [[ -n "${BACKR_TRUST_PUBKEY:-}" ]]; then
    while IFS= read -r line || [[ -n "${line:-}" ]]; do
      append_one_trust_pubkey_line_if_new "$ak" "$line" "$BACKR_USER"
    done <<<"${BACKR_TRUST_PUBKEY}"
  fi
}

#
# Inputs: backup account home. Outputs: SELinux ssh_home_t on ~/.ssh when SELinux is enforcing (inputs: paths only).
# External: restorecon fixes contexts on RHEL/Fedora-family when mcstrans/sshd enforce labeling.
#
selinux_restore_ssh_home_if_enforcing() {
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  local home_dir="$1"
  command -v selinuxenabled &>/dev/null || return 0
  selinuxenabled 2>/dev/null || return 0
  if command -v restorecon &>/dev/null; then
    echo "SELinux enforcing — restoring contexts on ${home_dir}/.ssh …"
    restorecon -Rv "${home_dir}/.ssh" 2>/dev/null || true
  fi
}

#
# Outputs: first candidate for the desktop user who should receive the Backr AppImage install.
# Priority: HOST_DESKTOP_USER global → $SUDO_USER env → first UID≥1000 user with a real home → dies when none found.
#
detect_desktop_user() {
  if [[ -n "${HOST_DESKTOP_USER:-}" ]]; then
    id "$HOST_DESKTOP_USER" &>/dev/null || die "desktop user '${HOST_DESKTOP_USER}' not found (--desktop-user / BACKR_HOST_DESKTOP_USER)"
    echo "$HOST_DESKTOP_USER"
    return 0
  fi
  if [[ -n "${SUDO_USER:-}" ]] && id "$SUDO_USER" &>/dev/null; then
    echo "$SUDO_USER"
    return 0
  fi
  # Fall back to the first local user with UID >= 1000 and a real home.
  local candidate=""
  while IFS=: read -r uname _ uid _ _ uhome _; do
    [[ "${uid:-0}" -ge 1000 ]] || continue
    [[ -d "${uhome:-}" ]] || continue
    candidate="$uname"
    break
  done </etc/passwd
  [[ -n "$candidate" ]] ||
    die "could not detect a desktop user; pass --desktop-user NAME or BACKR_HOST_DESKTOP_USER"
  echo "$candidate"
}

#
# Inputs: none — uses detect_pkg_backend. Outputs: installs Tauri's system-level build deps as root.
# Mirrors the Tauri prerequisite list from setup-connecting-client.sh.
#
install_host_tauri_system_deps() {
  local backend
  backend="$(detect_pkg_backend)"
  echo "Installing Tauri build dependencies (${backend}) …"
  case "$backend" in
    apt)
      apt-get update -qq
      apt-get install -y \
        build-essential pkg-config cmake mold curl wget git \
        libwebkit2gtk-4.1-dev libssl-dev \
        libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev file
      ;;
    dnf)
      dnf install -y \
        curl wget git openssl-devel mold gcc gcc-c++ make cmake pkgconf-pkg-config \
        webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel \
        perl-File-MimeInfo patch
      ;;
    pacman)
      pacman -Sy --noconfirm \
        base-devel curl wget git openssl mold \
        webkit2gtk-4.1 gtk3 libappindicator-gtk3 librsvg patchelf pkgconf cmake
      ;;
    zypper)
      zypper --non-interactive refresh
      zypper --non-interactive install -y \
        curl wget git openssl-devel gcc gcc-c++ cmake pkg-config patch \
        webkit2gtk4-devel gtk3-devel libayatana-appindicator-devel librsvg-devel libxdo-devel \
        2>/dev/null ||
        zypper --non-interactive install -y \
          curl wget git openssl-devel gcc gcc-c++ cmake pkg-config patch \
          webkit2gtk-devel gtk3-devel libappindicator-devel librsvg-devel
      ;;
    apk)
      apk add --no-cache \
        build-base curl wget git openssl-dev \
        webkit2gtk-dev gtk+3.0-dev librsvg-dev libayatana-indicator-dev bash file
      ;;
    *)
      echo "warning: unknown distro — Tauri system deps may be missing; build may fail" >&2
      ;;
  esac
}

#
# Inputs: $1 target username, $2 their home directory.
# Outputs: path to a built Backr.AppImage in a temp dir (caller must clean up the dir).
#          Installs Tauri system deps (root), downloads source tarball, installs Rust if needed,
#          builds via 'npm ci && npm run tauri:build' as the target user.
#          Prints progress to stdout; returns non-zero on failure.
#
build_host_appimage_from_source() {
  local target_user="$1" target_home="$2"

  echo "Building Backr from source (this takes ~10-20 min on first run) …"

  # Install system-level Tauri build deps (requires root, already running as root).
  install_host_tauri_system_deps

  # Node.js is already available from ensure_nodejs_for_host_survey() or system install.
  # Make sure it's present for npm.
  if ! command -v node &>/dev/null; then
    echo "Installing Node.js for build …"
    ensure_nodejs_for_host_survey
  fi
  echo "Node: $(node --version 2>/dev/null || echo 'not found')"

  # Download source tarball from GitHub.
  local src_dir
  src_dir="$(mktemp -d "${TMPDIR:-/tmp}/backr-src.XXXXXX")"
  local repo_slug=""
  repo_slug="$(echo "$BACKR_SCRIPTS_RAW_BASE" | sed -n 's|.*githubusercontent\.com/\([^/]*/[^/]*\)/.*|\1|p')"
  [[ -n "$repo_slug" ]] || repo_slug="$(echo "$BACKR_SCRIPTS_RAW_BASE" | sed -n 's|.*github\.com/\([^/]*/[^/]*\)/.*|\1|p')"
  [[ -n "$repo_slug" ]] || repo_slug="perfekt1406-hub/Backr"
  local tarball_url="https://github.com/${repo_slug}/archive/refs/heads/main.tar.gz"
  echo "Downloading source from ${tarball_url} …"
  if ! curl -fsSL "$tarball_url" | tar -xz -C "$src_dir" --strip-components=1; then
    rm -rf "$src_dir"
    echo "warning: failed to download source tarball from ${tarball_url}" >&2
    return 1
  fi

  # Hand ownership to the target user so Rust/npm write into their home.
  chown -R "${target_user}:${target_user}" "$src_dir"

  # Install Rust + build as the target user.
  # Uses 'npx tauri build' so the frontend is properly embedded in the binary.
  # Raw 'cargo build' does NOT embed frontend assets — the app would try to
  # connect to the dev server (localhost:1420) and fail.
  runuser -u "$target_user" -- bash -s <<USERSCRIPT
set -euo pipefail
export HOME="${target_home}"
export CARGO_HOME="\${CARGO_HOME:-\$HOME/.cargo}"
export RUSTUP_HOME="\${RUSTUP_HOME:-\$HOME/.rustup}"
export PATH="\$CARGO_HOME/bin:\$PATH"

if ! command -v cargo &>/dev/null; then
  echo "Installing Rust toolchain …"
  curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | \
    sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path
fi
[[ -f "\$HOME/.cargo/env" ]] && source "\$HOME/.cargo/env"
rustup default stable 2>/dev/null || true
echo "Rust: \$(rustc --version)"

cd "$src_dir"
echo "Installing npm deps …"
npm ci
echo "Building Backr (tauri build — Rust compile takes 10-20 min on first run) …"
npx tauri build
USERSCRIPT

  # On Arch/pacman systems, use the raw binary (no AppImage wrapper) to avoid
  # EGL/Mesa conflicts caused by linuxdeploy's LD_LIBRARY_PATH manipulation.
  local backend
  backend="$(detect_pkg_backend)"
  if [[ "$backend" == "pacman" ]]; then
    local raw_binary="${src_dir}/src-tauri/target/release/backr"
    if [[ -f "$raw_binary" ]]; then
      echo "Build complete (native binary): ${raw_binary}"
      echo "NATIVE:${raw_binary}"
      return 0
    fi
  fi

  local appimage
  appimage="$(find "$src_dir/src-tauri/target/release/bundle/appimage" -name "*.AppImage" 2>/dev/null | head -1)"
  if [[ -z "$appimage" ]]; then
    rm -rf "$src_dir"
    echo "warning: build completed but no AppImage found under src-tauri/target/release/bundle/appimage" >&2
    return 1
  fi

  echo "Build complete: ${appimage}"
  echo "$appimage"   # caller reads this line as the path
}

#
# Inputs: $1 HTTPS URL to an AppImage. Outputs: path to a downloaded temp file (caller must rm); non-zero on failure.
# External: curl fetches URL into a tempfile following redirects.
#
download_appimage_to_tempfile() {
  local url="$1"
  [[ -n "$url" ]] || die "internal: empty AppImage URL"
  command -v curl &>/dev/null || die "curl required to download the Backr AppImage"
  local tmp
  tmp="$(mktemp "${TMPDIR:-/tmp}/backr-host-appimage.XXXXXX")"
  # External: curl fetches URL into tmp with fail-on-HTTP-error and location following.
  if ! curl -fL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 -o "$tmp" "$url"; then
    rm -f "$tmp"
    return 1
  fi
  echo "$tmp"
}

#
# Inputs: $1 target username, $2 absolute path to their home directory.
# Outputs: copies Backr PNGs into the user's hicolor icon theme for common grid sizes.
# External: gtk-update-icon-cache refreshes the hicolor theme index when available.
#
install_host_backr_icon_to_user_theme() {
  local target_user="$1" target_home="$2"

  # Locate icons: prefer local repo checkout; fall back to downloading from raw GitHub.
  local script_dir=""
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd 2>/dev/null)" || script_dir=""
  local repo_icons="${script_dir}/../src-tauri/icons"
  local use_remote=0
  [[ -d "$repo_icons" ]] || use_remote=1

  local raw_base="${BACKR_SCRIPTS_RAW_BASE%/scripts}"

  local pair="" src="" dest_size="" icon_dir="" tmp_icon=""
  for pair in \
    "32x32.png|32x32" \
    "128x128.png|128x128" \
    "icon.png|256x256"; do
    dest_size="${pair##*|}"
    icon_dir="${target_home}/.local/share/icons/hicolor/${dest_size}/apps"
    mkdir -p "$icon_dir"

    if [[ "$use_remote" -eq 0 ]]; then
      src="${repo_icons}/${pair%%|*}"
      [[ -f "$src" ]] || continue
      install -m 644 -o "$target_user" -g "$target_user" "$src" \
        "${icon_dir}/com.backr.app.png"
    else
      # Download icon from raw GitHub when running piped from curl.
      tmp_icon="$(mktemp /tmp/backr-icon.XXXXXX.png)"
      if curl -fsSL "${raw_base}/src-tauri/icons/${pair%%|*}" -o "$tmp_icon" 2>/dev/null; then
        install -m 644 -o "$target_user" -g "$target_user" "$tmp_icon" \
          "${icon_dir}/com.backr.app.png"
      fi
      rm -f "$tmp_icon"
    fi
  done

  if command -v gtk-update-icon-cache &>/dev/null; then
    local hicolor="${target_home}/.local/share/icons/hicolor"
    gtk-update-icon-cache -f -t "$hicolor" &>/dev/null || true
  fi
}

#
# Inputs: $1 target username, $2 absolute path to their home directory.
# Outputs: refreshes XDG/GNOME/KDE application menu caches so the new .desktop entry is visible.
# External: update-desktop-database, kbuildsycoca5/6, xdg-desktop-menu — all best-effort, non-fatal.
#
refresh_host_application_launcher_caches() {
  local target_user="$1" target_home="$2"
  local apps_dir="${target_home}/.local/share/applications"
  [[ -d "$apps_dir" ]] || return 0

  # Run cache-refresh commands as the desktop user so they read/write user-owned cache files.
  if command -v update-desktop-database &>/dev/null; then
    # External: update-desktop-database indexes user's ~/.local/share/applications (inputs: dir; outputs: mimeinfo.cache).
    runuser -u "$target_user" -- update-desktop-database "$apps_dir" &>/dev/null || true
  fi
  if command -v kbuildsycoca6 &>/dev/null; then
    runuser -u "$target_user" -- kbuildsycoca6 --noincremental &>/dev/null || true
  elif command -v kbuildsycoca5 &>/dev/null; then
    runuser -u "$target_user" -- kbuildsycoca5 --noincremental &>/dev/null || true
  fi
  if command -v xdg-desktop-menu &>/dev/null; then
    runuser -u "$target_user" -- xdg-desktop-menu forceupdate &>/dev/null || true
  fi
}

#
# Inputs: $1 target username, $2 their home directory.
# Outputs: writes the host-mode .desktop entry to ~/.local/share/applications/ and refreshes
#          launcher caches. Safe to call before the AppImage is downloaded — TryExec is omitted
#          so the entry is always visible in the app menu even when the binary is not yet present.
#
install_host_desktop_entry() {
  local target_user="$1" target_home="$2"
  local dest_dir="${target_home}/.local/share/backr"
  # Use raw binary on Arch (avoids AppImage LD_LIBRARY_PATH / EGL issues).
  local dest
  local backend
  backend="$(detect_pkg_backend)"
  if [[ "$backend" == "pacman" ]]; then
    dest="${dest_dir}/backr"
  else
    dest="${dest_dir}/Backr.AppImage"
  fi
  local desktop_dir="${target_home}/.local/share/applications"
  local desktop="${desktop_dir}/com.backr.app.desktop"

  mkdir -p "$dest_dir" "$desktop_dir"
  install_host_backr_icon_to_user_theme "$target_user" "$target_home"

  # TryExec is intentionally omitted — some launchers hide entries whose TryExec binary
  # is missing. Omitting it means the entry always appears; clicking it before the
  # AppImage is downloaded will simply do nothing.
  # WEBKIT_DISABLE_DMABUF_RENDERER=1 prevents white/blank windows on Wayland
  # (WebKitGTK DMA-BUF framebuffer failures on rolling-release Mesa).
  cat >"$desktop" <<EOF
[Desktop Entry]
Version=1.5
Type=Application
Name=Backr (Host Dashboard)
GenericName=Backup host dashboard
Comment=Backr host-dashboard — inspect backups and trust client keys (rsync over SSH)
Exec=env BACKR_HOST_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 ${dest} %u
Icon=com.backr.app
Terminal=false
Categories=Utility;Archiving;Network;
Keywords=backup;rsync;snapshot;Backr;ssh;host;
StartupNotify=true
StartupWMClass=com.backr.app
EOF
  chown "${target_user}:${target_user}" "$desktop"
  chmod 644 "$desktop"

  refresh_host_application_launcher_caches "$target_user" "$target_home"
  echo "Launcher entry written: ${desktop}"
  echo "  → Search «Backr» in your app menu to open the host dashboard."
}

#
# Inputs: $1 absolute path to the downloaded AppImage, $2 target username, $3 their home directory.
# Outputs: copies AppImage to ~/.local/share/backr/Backr.AppImage (mode 755, owned by target_user).
#
install_host_appimage_binary() {
  local src="$1" target_user="$2" target_home="$3"
  [[ -f "$src" ]] || die "AppImage not found: $src"
  local dest_dir="${target_home}/.local/share/backr"
  local dest="${dest_dir}/Backr.AppImage"
  mkdir -p "$dest_dir"
  install -m 755 -o "$target_user" -g "$target_user" "$src" "$dest"
  echo "Installed Backr AppImage: ${dest}"
}

#
# Inputs: HOST_APPIMAGE_URL, HOST_DESKTOP_USER / SUDO_USER (via detect_desktop_user).
# Outputs: always installs the .desktop launcher entry; also downloads the AppImage when
#          the URL is reachable. Download failure is non-fatal — a manual recovery command
#          is printed and setup continues.
#
install_host_appimage_runtime_deps() {
  local backend
  backend="$(detect_pkg_backend)"
  echo "Ensuring WebKitGTK runtime dependencies for Backr AppImage (${backend}) …"
  case "$backend" in
    apt)
      apt-get update -qq
      apt-get install -y libwebkit2gtk-4.1-0 libayatana-appindicator3-1 librsvg2-2 libfuse2 2>/dev/null || true
      ;;
    dnf)
      dnf install -y webkit2gtk4.1 libappindicator-gtk3 librsvg2 fuse-libs 2>/dev/null || true
      ;;
    pacman)
      pacman -Sy --noconfirm webkit2gtk-4.1 libappindicator-gtk3 librsvg fuse2 2>/dev/null || true
      ;;
    zypper)
      zypper --non-interactive install webkit2gtk-4_1 libappindicator3-1 librsvg-2 libfuse2 2>/dev/null || true
      ;;
    *)
      echo "  ⚠ Unknown package manager — ensure webkit2gtk-4.1 is installed manually."
      ;;
  esac
}

install_host_app_from_appimage_url() {
  [[ -n "$HOST_APPIMAGE_URL" ]] || die "internal: install_host_app_from_appimage_url called without a URL"

  local target_user target_home tmp
  target_user="$(detect_desktop_user)"
  target_home="$(getent passwd "$target_user" | cut -d: -f6)"
  [[ -n "$target_home" ]] || die "could not resolve home directory for user '${target_user}'"
  [[ -d "$target_home" ]] || die "home directory '${target_home}' for user '${target_user}' does not exist"

  echo "Installing Backr host-dashboard app for user '${target_user}' (home: ${target_home}) …"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "[dry-run] download ${HOST_APPIMAGE_URL}"
    echo "[dry-run] install AppImage + .desktop for ${target_user} at ${target_home}"
    return 0
  fi

  # WebKitGTK is required at runtime but not bundled in the AppImage.
  install_host_appimage_runtime_deps

  # Always write the .desktop entry so the app appears in the launcher immediately.
  install_host_desktop_entry "$target_user" "$target_home"

  local appimage_src=""
  local built_src_dir=""

  # Pre-built AppImages are compiled on Debian/Ubuntu and bundle EGL/Mesa stubs
  # that are incompatible with Arch's newer Mesa stack (causes white screen / EGL_BAD_PARAMETER).
  # On pacman-based systems, always build from source to use the native WebKitGTK + Mesa.
  local backend
  backend="$(detect_pkg_backend)"
  local force_source=0
  if [[ "$backend" == "pacman" ]]; then
    echo "Arch-based system detected — building from source for WebKitGTK/Mesa compatibility …"
    force_source=1
  fi

  # On Arch: build native binary, install directly (no AppImage wrapper).
  # AppImages bundle EGL/Mesa stubs compiled on Debian that conflict with
  # Arch's rolling Mesa — a native build links against the system WebKitGTK.
  if [[ "$force_source" -eq 1 ]]; then
    local dest_dir dest
    dest_dir="${target_home}/.local/share/backr"
    dest="${dest_dir}/backr"

    # A re-run always reinstalls/updates: rebuild from the latest source and
    # replace any existing binary instead of skipping.  Skipping a present binary
    # would (a) never pick up updates and (b) risk keeping a stale or dev-mode
    # build — one that loads the vite devUrl http://localhost:1420 and shows
    # "connection refused" on a host with no dev server.  To re-run only for the
    # SSH/firewall setup without the 10-20 min rebuild, pass --no-appimage /
    # BACKR_NO_HOST_APPIMAGE=1.
    if [[ -x "$dest" ]]; then
      echo "Existing Backr binary found at ${dest} — rebuilding from latest source to reinstall/update …"
    fi
    # Remove the old binary and the now-obsolete .tauri-built marker left by
    # earlier script versions; the fresh build below replaces the binary.
    rm -f "$dest" "${dest_dir}/.tauri-built"

    install_host_tauri_system_deps

    if ! command -v node &>/dev/null; then
      ensure_nodejs_for_host_survey
    fi

    # Always build in a temp dir.  Using the live repo directly would require
    # chown -R (breaks git for the original user) and pollutes the source tree
    # with build artifacts.
    local src_dir
    src_dir="$(mktemp -d "${TMPDIR:-/tmp}/backr-src.XXXXXX")"

    # Prefer copying from a local git clone when running from the repo.
    local script_dir=""
    if [[ -n "${BASH_SOURCE[0]:-}" ]] && [[ -f "${BASH_SOURCE[0]}" ]] && [[ "${BASH_SOURCE[0]}" != /dev/* ]]; then
      script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    fi
    if [[ -n "$script_dir" ]] && [[ -f "${script_dir}/../src-tauri/Cargo.toml" ]]; then
      local repo_root
      repo_root="$(cd "${script_dir}/.." && pwd)"
      echo "Copying local repo source to build directory …"
      # Copy source files only — exclude heavy dirs that would slow things down.
      rsync -a --exclude='node_modules' --exclude='.git' --exclude='target' \
        "${repo_root}/" "${src_dir}/"
    else
      # BACKR_SCRIPTS_RAW_BASE uses raw.githubusercontent.com which cannot serve
      # archives; derive the proper github.com archive URL from the repo slug.
      local repo_slug=""
      repo_slug="$(echo "$BACKR_SCRIPTS_RAW_BASE" | sed -n 's|.*githubusercontent\.com/\([^/]*/[^/]*\)/.*|\1|p')"
      [[ -n "$repo_slug" ]] || repo_slug="$(echo "$BACKR_SCRIPTS_RAW_BASE" | sed -n 's|.*github\.com/\([^/]*/[^/]*\)/.*|\1|p')"
      [[ -n "$repo_slug" ]] || repo_slug="perfekt1406-hub/Backr"
      local tarball_url="https://github.com/${repo_slug}/archive/refs/heads/main.tar.gz"
      echo "Downloading Backr source from ${tarball_url} …"
      if ! curl -fsSL "$tarball_url" | tar -xz -C "$src_dir" --strip-components=1; then
        rm -rf "$src_dir"
        echo "error: failed to download source from ${tarball_url}" >&2
        SKIP_HOST_APPIMAGE=1
        return 1
      fi
    fi

    chown -R "${target_user}:${target_user}" "$src_dir"

    echo ""
    echo "Building Backr from source …"
    echo "  Step 1/2: npm ci (installing JS dependencies)"
    echo "  Step 2/2: tauri build --no-bundle (frontend + Rust compile — 10-20 min on first run)"
    echo ""
    runuser -u "$target_user" -- bash -c "
      export HOME='${target_home}'
      export CARGO_HOME=\"\${CARGO_HOME:-\$HOME/.cargo}\"
      export RUSTUP_HOME=\"\${RUSTUP_HOME:-\$HOME/.rustup}\"
      export PATH=\"\$CARGO_HOME/bin:\$PATH\"
      if ! command -v cargo &>/dev/null; then
        echo 'Installing Rust toolchain …'
        curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path
      fi
      [[ -f \"\$HOME/.cargo/env\" ]] && source \"\$HOME/.cargo/env\"
      cd '$src_dir'
      echo '── Step 1/2: npm ci ──'
      npm ci
      echo '── Step 2/2: tauri build --no-bundle (frontend + Rust — 10-20 min on first run) ──'
      mkdir -p \"\$HOME/.cache/tauri\"
      npx tauri build --no-bundle
      echo '── Build complete ──'
    " || {
      echo "error: source build failed." >&2
      rm -rf "$src_dir"
      SKIP_HOST_APPIMAGE=1
      return 1
    }

    local built_bin="${src_dir}/src-tauri/target/release/backr"
    if [[ ! -f "$built_bin" ]]; then
      echo "error: build completed but binary not found at ${built_bin}" >&2
      rm -rf "$src_dir"
      SKIP_HOST_APPIMAGE=1
      return 1
    fi

    mkdir -p "$dest_dir"
    install -m 755 -o "$target_user" -g "$target_user" "$built_bin" "$dest"
    echo "Installed native binary: ${dest}"
    rm -rf "$src_dir"
    return 0
  fi

  # Non-Arch: download pre-built AppImage.
  if tmp="$(download_appimage_to_tempfile "$HOST_APPIMAGE_URL" 2>/dev/null)"; then
    appimage_src="$tmp"
    trap 'rm -f "$appimage_src"' RETURN
  else
    echo "Pre-built AppImage not available at ${HOST_APPIMAGE_URL} — falling back to source build …"
    local build_out
    build_out="$(build_host_appimage_from_source "$target_user" "$target_home")" || {
      echo "warning: source build failed — the launcher entry is installed but the binary is missing." >&2
      SKIP_HOST_APPIMAGE=1
      return 1
    }
    appimage_src="$(echo "$build_out" | tail -1)"
    built_src_dir="$(dirname "$(dirname "$(dirname "$(dirname "$appimage_src")")")")"
    trap 'rm -rf "$built_src_dir"' RETURN
  fi

  install_host_appimage_binary "$appimage_src" "$target_user" "$target_home"
}

#
# Inputs: none — uses detect_desktop_user and the installed AppImage path.
# Outputs: launches the Backr host-dashboard app as the desktop user in the background.
#          Detects Wayland or X11 session from the user's XDG_RUNTIME_DIR / DISPLAY.
#          Silent no-op when no graphical session is found (headless hosts).
#
launch_host_dashboard_app() {
  [[ "$DRY_RUN" -eq 1 ]] && { echo "[dry-run] launch Backr host-dashboard app"; return 0; }

  local target_user target_home target_uid dest xdg_runtime
  target_user="$(detect_desktop_user 2>/dev/null)" || return 0
  target_home="$(getent passwd "$target_user" | cut -d: -f6)"
  target_uid="$(id -u "$target_user" 2>/dev/null)" || return 0
  # Check for native binary first (Arch), then AppImage.
  dest="${target_home}/.local/share/backr/backr"
  if [[ ! -f "$dest" ]]; then
    dest="${target_home}/.local/share/backr/Backr.AppImage"
  fi
  [[ -f "$dest" ]] || { echo "warning: Backr binary not found — skipping auto-launch" >&2; return 0; }

  xdg_runtime="/run/user/${target_uid}"

  # D-Bus address: try the user's actual session bus first (avoids assuming the
  # socket lives at the systemd default path, which varies across session managers).
  local dbus_addr=""
  local dbus_sock="${xdg_runtime}/bus"
  if [[ -S "$dbus_sock" ]]; then
    dbus_addr="unix:path=${dbus_sock}"
  fi

  # Build the display environment: prefer Wayland (any wayland-* socket), fall back to X11.
  # Hyprland and some other compositors use wayland-1 rather than wayland-0.
  local display_args=()
  local wayland_sock=""
  for sock in "${xdg_runtime}"/wayland-*; do
    [[ -S "$sock" ]] && { wayland_sock="${sock##*/}"; break; }
  done

  # Common env vars needed by every graphical launch path.
  # HOME is critical — runuser without -l inherits root's HOME, causing
  # WebKitGTK profile/cache failures and silent crashes.
  # WEBKIT_DISABLE_DMABUF_RENDERER prevents white/blank windows on Wayland
  # (DMA-BUF framebuffer failures with rolling-release Mesa + WebKitGTK).
  local common_args=(
    "HOME=${target_home}"
    "XDG_RUNTIME_DIR=${xdg_runtime}"
    "WEBKIT_DISABLE_DMABUF_RENDERER=1"
  )
  [[ -n "$dbus_addr" ]] && common_args+=("DBUS_SESSION_BUS_ADDRESS=${dbus_addr}")

  if [[ -n "$wayland_sock" ]]; then
    display_args=(
      "${common_args[@]}"
      "WAYLAND_DISPLAY=${wayland_sock}"
      "XDG_SESSION_TYPE=wayland"
    )
  elif [[ -n "${DISPLAY:-}" ]]; then
    display_args=(
      "${common_args[@]}"
      "DISPLAY=${DISPLAY}"
    )
  elif [[ -S "/tmp/.X11-unix/X0" ]]; then
    display_args=(
      "${common_args[@]}"
      "DISPLAY=:0"
    )
  else
    echo "No graphical session detected for '${target_user}' — skipping auto-launch. Open Backr from the app menu when logged in."
    return 0
  fi

  echo "Launching Backr host dashboard for '${target_user}' …"
  # Each launch truncates (not appends) the log so diagnostics reflect only the
  # current run.  Appending across runs previously mixed stale output — a benign
  # "disabling dmabuf renderer" notice and old GTK warnings — into later reads,
  # which caused the launch failure to be misdiagnosed as a rendering/dmabuf bug.
  local launch_log="${target_home}/.local/share/backr/launch.log"
  mkdir -p "$(dirname "$launch_log")"
  # A re-run reinstalls a fresh binary, so stop any previously running instance
  # first — otherwise tauri-plugin-single-instance would just focus the OLD
  # version and the update would appear not to take effect.  Best-effort: this
  # script runs as root, so the signal reaches the desktop user's process.
  if command -v pkill &>/dev/null; then
    pkill -x backr 2>/dev/null || true
    sleep 1   # let it release the single-instance lock before relaunching
  fi
  # Close stdin so the app doesn't inherit the script's /dev/tty fd (from
  # the questionnaire's exec </dev/tty).  Without this, the shell waits for
  # all processes sharing that fd to exit before returning the prompt.
  # Do NOT use setsid — it creates a new session that disconnects the process
  # from the Wayland compositor's session tracking, preventing window creation.
  runuser -u "$target_user" -- env \
    "${display_args[@]}" \
    BACKR_HOST_MODE=1 \
    "$dest" </dev/null >"$launch_log" 2>&1 &
  local launch_pid=$!
  disown "$launch_pid" 2>/dev/null || true

  # Give the process a moment to crash, start, or hand off; report accurately.
  # Backr uses tauri-plugin-single-instance: when an instance is already running,
  # the freshly-spawned launcher focuses the existing window and exits 0.  That
  # clean hand-off must not be reported as a crash — so before crying failure we
  # check whether a Backr process is actually running.
  sleep 2
  if kill -0 "$launch_pid" 2>/dev/null; then
    echo "Backr host dashboard launched (PID ${launch_pid})."
  elif command -v pgrep &>/dev/null && pgrep -x backr &>/dev/null; then
    echo "Backr is already running — brought the existing window to the front."
  else
    echo "warning: Backr process exited immediately — check ${launch_log} for details." >&2
    tail -n 20 "$launch_log" 2>/dev/null | head -n 10 >&2 || true
  fi
}

#
# Writes `/etc/backr/host.toml` so Backr can open host-dashboard mode on this machine without a client config.
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
  # This marker is what makes Backr open in HOST (dashboard) mode here; the client
  # installer removes it so a client box opens in client mode (last setup wins).
  echo "Wrote host marker ${f} — Backr opens in HOST (dashboard) mode on this machine."
}

#
# Inputs: none. Outputs: sanity checks after setup (sshd -t, rsync binary). Warns if sshd process not seen.
#
verify_backup_host_ready() {
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  sshd -t || die "final sshd -t failed — do not rely on SSH until this is fixed"
  command -v rsync &>/dev/null || die "rsync command missing after package install"
  if ! pgrep -x sshd &>/dev/null && ! pgrep -x ssh &>/dev/null; then
    echo "warning: could not find sshd in process list — check service status if backups fail to connect" >&2
  fi
}

#
# Inputs: none. Outputs: prints OS/firewall/sshd facts discovered on this machine (no extra questions asked).
# External: sshd -T prints effective merged configuration (inputs: none when run as root; outputs: key value lines).
#
report_detected_ssh_environment() {
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  local home="" ak="" n_keys="" pkg=""
  echo ""
  echo "── Auto-detected (scripts infer this; you don’t need to answer questionnaires) ──"
  if [[ -f /etc/os-release ]]; then
    # shellcheck source=/dev/null
    . /etc/os-release
    echo "  OS: ${PRETTY_NAME:-${NAME:-${ID:-unknown}}}"
  fi
  pkg="$(detect_pkg_backend)"
  echo "  Package backend: ${pkg}"

  if command -v selinuxenabled &>/dev/null && selinuxenabled 2>/dev/null; then
    echo -n "  SELinux: enforcing"
    if command -v getenforce &>/dev/null; then
      echo " ($(getenforce 2>/dev/null))"
    else
      echo ""
    fi
  elif command -v getenforce &>/dev/null; then
    echo "  SELinux: $(getenforce 2>/dev/null || echo unknown)"
  fi

  if command -v ufw &>/dev/null; then
    if ufw status 2>/dev/null | grep -qiE 'Status:[[:space:]]*active'; then
      echo "  Firewall: UFW active"
    else
      echo "  Firewall: UFW present, not active"
    fi
  fi
  if command -v firewall-cmd &>/dev/null; then
    if systemctl is-active --quiet firewalld 2>/dev/null; then
      echo "  Firewall: firewalld active"
    else
      echo "  Firewall: firewalld installed, not active"
    fi
  fi

  if command -v sshd &>/dev/null; then
    echo -n "  sshd Port (effective): "
    sshd -T 2>/dev/null | grep -i '^port ' | awk '{printf "%s ", $2}' || printf '(could not run sshd -T)'
    echo ""
    echo -n "  sshd PasswordAuthentication (effective global): "
    sshd -T 2>/dev/null | grep -i '^passwordauthentication ' || echo "(unknown)"
    echo -n "  sshd PubkeyAuthentication (effective global): "
    sshd -T 2>/dev/null | grep -i '^pubkeyauthentication ' || echo "(unknown)"
  fi

  home="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  ak="${home}/.ssh/authorized_keys"
  n_keys="$(count_pubkey_lines_in_authorized_keys "$ak")"
  echo "  ${BACKR_USER} authorized_keys pubkey lines: ${n_keys}"

  if command -v ss &>/dev/null; then
    echo "  TCP listeners (ssh/sshd processes):"
    ss -tlnp 2>/dev/null | grep -E '(:22\b|:ssh\b)' | head -n 16 || ss -tlnp 2>/dev/null | grep sshd | head -n 16 || ss -tlnp 2>/dev/null | head -n 12 || true
  fi

  cat <<'NOTE'

  Scripts cannot see router port-forwards or cloud security groups from here — pair this section with your LAN/router/VPN docs.

NOTE
}

#
# Inputs: globals BACKR_USER, BACKR_ROOT. Outputs: brief summary row plus SSH smoke-test hint (passwordless **backr** when keys exist).
#
print_host_ready() {
  local ip_line=""
  # External: hostname gathers local identity strings for display (inputs: flags; outputs: stdout).
  if command -v hostname &>/dev/null; then
    ip_line="$(hostname -I 2>/dev/null | awk '{print $1}' || true)"
  fi
  cat <<EOF

── Backr backup host ready ──
  SSH user:       ${BACKR_USER}
  Backup root:    ${BACKR_ROOT}
  Host marker:    /etc/backr/host.toml
EOF
  if [[ -n "$ip_line" ]]; then
    printf '  Primary IP:     %s (run ip addr if this box has several interfaces)\n' "$ip_line"
  fi
  if [[ "${SKIP_HOST_APPIMAGE:-0}" != "1" ]]; then
    local du="" bin_name="Backr.AppImage"
    du="$(detect_desktop_user 2>/dev/null || echo '(desktop-user)')"
    [[ "$(detect_pkg_backend)" == "pacman" ]] && bin_name="backr"
    printf '  Host dashboard: ~/.local/share/backr/%s (installed for %s)\n' "$bin_name" "$du"
    printf '                  → It should open automatically. If it did not, search «Backr» in\n'
    printf '                    your app menu or run: ~/.local/share/backr/%s\n' "$bin_name"
  else
    printf '  Host dashboard: skipped (--no-appimage). Use authorized_keys to add client pubkeys.\n'
  fi
  cat <<EOF

  → Open Backr and use «Trust keys → Add a laptop» to connect a laptop in one tap.

EOF
}

#
# Adds nss-mdns to /etc/nsswitch.conf's hosts line when absent so glibc (and thus ssh/
# rsync) resolves `.local` names. Inserts `mdns4_minimal [NOTFOUND=return]` right after
# `files`. Debian's libnss-mdns auto-configures this; Arch and others do not.
# Inputs: none. Outputs: edits /etc/nsswitch.conf in-place or warns on failure.
#
ensure_nsswitch_mdns() {
  local f=/etc/nsswitch.conf
  [[ -f "$f" ]] || { echo "warning: ${f} missing; cannot enable mdns resolution." >&2; return 0; }
  if grep -qE '^hosts:.*mdns' "$f"; then
    echo "nsswitch.conf already resolves mdns."
    return 0
  fi
  sed -i -E '/^hosts:/{ /mdns/! s/\bfiles\b/files mdns4_minimal [NOTFOUND=return]/ }' "$f" \
    && echo "Added mdns4_minimal to ${f} hosts line." \
    || echo "warning: could not edit ${f}; add 'mdns4_minimal [NOTFOUND=return]' to the hosts line manually." >&2
}

#
# Enables and starts the avahi daemon (systemd or OpenRC) so this host publishes and
# resolves `.local` mDNS names. Best-effort: warns instead of failing setup.
# Inputs: none. Outputs: enables/starts avahi-daemon or prints a warning.
#
enable_avahi_daemon() {
  if command -v systemctl &>/dev/null; then
    systemctl enable --now avahi-daemon 2>/dev/null \
      || systemctl enable --now avahi-daemon.service 2>/dev/null \
      || echo "warning: could not enable avahi-daemon; start it manually so the .local name is published." >&2
  elif command -v rc-update &>/dev/null; then
    rc-update add avahi-daemon default 2>/dev/null || true
    rc-service avahi-daemon start 2>/dev/null || true
  fi
}

#
# Ensures this host publishes its `.local` mDNS name (e.g. archlinux.local) so paired
# laptops can reach it by name and keep backing up across DHCP IP changes. Installs and
# enables avahi (+ nss-mdns so the host resolves `.local` too) and adds mdns to nsswitch
# on distros that don't auto-configure it. Best-effort: warns instead of failing setup.
# Inputs: none. Outputs: installs packages via detect_pkg_backend, then calls
#         ensure_nsswitch_mdns and enable_avahi_daemon.
#
ensure_host_mdns_publish() {
  local backend
  backend="$(detect_pkg_backend)"
  echo "Ensuring this host publishes its .local mDNS name (so laptops follow IP changes) …"
  case "$backend" in
    apt)    run_cmd apt-get install -y avahi-daemon libnss-mdns || true ;;
    dnf)    run_cmd dnf install -y avahi nss-mdns || true ;;
    yum)    run_cmd yum install -y avahi nss-mdns || true ;;
    pacman) run_cmd pacman -S --noconfirm --needed avahi nss-mdns || true ;;
    zypper) run_cmd zypper --non-interactive install -y avahi nss-mdns || true ;;
    apk)    run_cmd apk add --no-cache avahi avahi-tools || true ;;
    *)      echo "warning: unknown package backend for mDNS; laptops may need to reach this host by IP." >&2 ;;
  esac

  [[ "$DRY_RUN" -eq 1 ]] && return 0

  ensure_nsswitch_mdns
  enable_avahi_daemon
}

main() {
  parse_args "$@"
  require_linux_root
  normalize_root
  run_backup_host_questionnaire

  install_server_ssh_rsync "$DRY_RUN"
  ensure_host_mdns_publish
  ensure_sshd_includes_drop_in_dir
  ensure_user_exists
  ensure_backup_tree
  ensure_ssh_dir
  append_trust_pubkeys_from_cli_or_env
  sshd_write_backr_drop_in "$DRY_RUN"

  local home_for_selinux
  home_for_selinux="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  [[ -n "$home_for_selinux" ]] && selinux_restore_ssh_home_if_enforcing "$home_for_selinux"

  open_ssh_on_active_managed_firewalls
  write_host_marker

  # Download and install the Backr host-dashboard app by default.
  # Use --no-appimage / BACKR_NO_HOST_APPIMAGE=1 to skip on headless servers.
  if [[ "${SKIP_HOST_APPIMAGE:-0}" != "1" ]]; then
    # --appimage-url / BACKR_HOST_APPIMAGE_URL overrides the default release URL.
    [[ -n "$HOST_APPIMAGE_URL" ]] || HOST_APPIMAGE_URL="$BACKR_DEFAULT_APPIMAGE_URL"
    # Non-fatal: download failure prints a warning and sets SKIP_HOST_APPIMAGE=1.
    if install_host_app_from_appimage_url; then
      launch_host_dashboard_app
    fi
  fi

  verify_backup_host_ready
  [[ "${VERBOSE:-0}" -eq 1 ]] && report_detected_ssh_environment
  print_host_ready
}

main "$@"
