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
SKIP_FIREWALL=0
# Optional pubkey bootstrap (see append_trust_pubkeys_from_cli_or_env).
TRUST_PUBKEY_FILE_CLI=""
# Questionnaire / non-interactive (see run_backup_host_questionnaire / emit_backup_host_custom_next_steps).
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
# Outputs: fills SURVEY_* globals used by emit_backup_host_custom_next_steps (sources a temp env file from Node).
# External: node runs scripts/backr-host-survey.mjs; npm installs @clack/prompts in a temp dir; curl may fetch the mjs from BACKR_SCRIPTS_RAW_BASE.
#
run_backup_host_questionnaire() {
  [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] && return 0
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  if ! survey_tty_is_usable; then
    SURVEY_SKIP_NO_TTY=1
    return 0
  fi

  # curl … | sudo bash leaves stdin on a pipe — attach stdin to the real terminal so Clack reads behave consistently.
  if [[ ! -t 0 ]]; then
    exec </dev/tty 2>/dev/null || true
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
}

#
# Inputs: SURVEY_* answers and runtime detection already printed. Outputs: tailored «what to do next» for unknowns.
#
emit_backup_host_custom_next_steps() {
  [[ "$DRY_RUN" -eq 1 ]] && return 0

  local eff_ports=""
  if command -v sshd &>/dev/null; then
    eff_ports="$(sshd -T 2>/dev/null | grep -i '^port ' | awk '{printf "%s ", $2}' | sed 's/[[:space:]]*$//')"
  fi

  # Gather LAN IPs once — used in the LAN-specific numbered checklist.
  local lan_ips="" primary_ip=""
  if command -v hostname &>/dev/null; then
    lan_ips="$(hostname -I 2>/dev/null | tr ' ' '\n' | grep -v '^$' | head -n 4 | tr '\n' ' ' | sed 's/[[:space:]]*$//' || true)"
  fi
  primary_ip="$(echo "$lan_ips" | awk '{print $1}')"

  echo ""
  echo "── Your next steps (based on your questionnaire + this machine) ──"

  if [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]]; then
    cat <<'NXT'
You used --non-interactive / BACKR_NON_INTERACTIVE — questionnaire was skipped.

  • If this was curl | sudo bash: run again from an interactive shell without --non-interactive if you want tailored hints.
  • Trust path (passwordless backr): paste laptop pubkeys via Backr → Trust keys (#/host/trust), edit ~BACKR_USER/.ssh/authorized_keys over SSH, or re-run with --trust-pubkey-file / BACKR_TRUST_PUBKEY to append automatically.
  • Clients must reach the sshd Port shown above (same for LAN router/VPN/firewall rules).

NXT
    return 0
  fi

  if [[ "${SURVEY_SKIP_NO_TTY:-0}" == "1" ]]; then
    cat <<'NXT'
No usable interactive terminal — questionnaire was skipped (common with SSH without a TTY, serial/IPMI consoles, Docker without `-it`, or some cloud agents).

  • Prefer an ordinary login shell: `ssh -t user@HOST`, then `curl … | sudo bash` from there — or clone the repo and run `sudo bash scripts/setup-backup-host.sh`.
  • Add `--non-interactive` when you intentionally want zero prompts on headless installs.
  • Interactive setup needs Node.js 18+ and runs a short @clack/prompts wizard (installed automatically when possible).

NXT
  fi

  # ── Reach-specific guidance ──────────────────────────────────────────────────────────────

  case "$SURVEY_REACH" in

    lan_only)
      cat <<EOF
• LAN-only path — checklist:

    1. This machine's LAN address(es): ${lan_ips:-run 'ip addr' to find them}
       SSH port: ${eff_ports:-22}

    2. On each laptop, clone the repo and run:
         ./scripts/setup-connecting-client.sh
       The wizard will ask for this machine's IP/hostname and SSH port.
       It installs deps, builds the AppImage, adds the app launcher, and at the
       end automatically runs ssh-copy-id to trust the laptop's key on this machine
       (you type the ${BACKR_USER} account password once at the prompt).

    3. Open Backr on the laptop and complete the in-app setup wizard.

  Note: backups only run while the laptop is on this same LAN — expected behaviour.

EOF
      ;;

    internet)
      printf '%s\n' "• Effective sshd TCP ports here: ${eff_ports:-unknown} (full detail above)."
      cat <<'NXT'
• Internet exposure: confirm your port-forward / cloud security group allows inbound TCP to the sshd port above; key-only backr is enforced automatically once authorized_keys has at least one key.

NXT
      ;;

    vpn)
      printf '%s\n' "• Effective sshd TCP ports here: ${eff_ports:-unknown} (full detail above)."
      cat <<'NXT'
• VPN path: document the VPN endpoint for laptops; SSH targets are usually private IPs visible only while VPN is up. Use --ssh-port on the client script when sshd is not on port 22.

NXT
      ;;

    unknown | *)
      printf '%s\n' "• Effective sshd TCP ports here: ${eff_ports:-unknown} (full detail above)."
      cat <<'NXT'
• You weren't sure how clients reach SSH. Check both paths:
    LAN:      ping this host's private IP from another device on the same network, then: nc -vz HOST 22 (or your SSH port).
    Internet: ensure your router forwards the SSH port to this machine and/or open the port in your cloud security group.
  If only VPN works, connect VPN first on the laptop before testing SSH.

NXT
      ;;
  esac

  # ── Key-trust guidance for non-LAN paths (LAN block above is self-contained) ────────────

  if [[ "$SURVEY_REACH" != "lan_only" ]]; then
    case "$SURVEY_KEYPATH" in
      backr_trust_ui)
        cat <<'NXT'
• Trust keys: open Backr on this machine → sidebar «Trust keys» (#/host/trust) → paste one full line from the laptop's ~/.ssh/id_ed25519.pub.

NXT
        ;;
      console_later)
        cat <<EOF
• Manual key install: append one line from the laptop's ~/.ssh/id_ed25519.pub to ~${BACKR_USER}/.ssh/authorized_keys on this machine (file mode 600; .ssh directory mode 700).

EOF
        ;;
      other_admin | unknown)
        cat <<EOF
• Coordinate with whoever admins SSH on this box — they need each laptop's single-line pubkey added to ${BACKR_USER}'s authorized_keys.

EOF
        ;;
    esac
  fi

  echo ""
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
        build-essential pkg-config cmake curl wget git \
        libwebkit2gtk-4.1-dev libssl-dev \
        libayatana-appindicator3-dev librsvg2-dev \
        libxdo-dev file
      ;;
    dnf)
      dnf install -y \
        curl wget git openssl-devel gcc gcc-c++ make cmake pkgconf-pkg-config \
        webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel \
        perl-File-MimeInfo patch
      ;;
    pacman)
      pacman -Sy --noconfirm \
        base-devel curl wget git openssl \
        webkit2gtk gtk3 libappindicator-gtk3 librsvg patchelf pkgconf cmake
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

  echo "No pre-built AppImage available — building Backr from source (this takes ~10-20 min on first run) …"

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
  local tarball_url="${BACKR_SCRIPTS_RAW_BASE%/scripts}/archive/refs/heads/main.tar.gz"
  echo "Downloading source from ${tarball_url} …"
  if ! curl -fsSL "$tarball_url" | tar -xz -C "$src_dir" --strip-components=1; then
    rm -rf "$src_dir"
    echo "warning: failed to download source tarball from ${tarball_url}" >&2
    return 1
  fi

  # Hand ownership to the target user so Rust/npm write into their home.
  chown -R "${target_user}:${target_user}" "$src_dir"

  # Install Rust + build the AppImage as the target user.
  # runuser executes a bash subshell that handles ~/.cargo/env sourcing internally.
  runuser -u "$target_user" -- bash -s <<USERSCRIPT
set -euo pipefail
export CARGO_HOME="\${CARGO_HOME:-\$HOME/.cargo}"
export RUSTUP_HOME="\${RUSTUP_HOME:-\$HOME/.rustup}"
export PATH="\$CARGO_HOME/bin:\$PATH"

# Install Rust via rustup when cargo is missing.
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
echo "Building AppImage (Rust compile — please wait) …"
npm run tauri:build
USERSCRIPT

  # Find the produced AppImage.
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
  # Icons shipped alongside this script in the repo's src-tauri/icons directory.
  local script_dir=""
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  local repo_icons="${script_dir}/../src-tauri/icons"

  [[ -d "$repo_icons" ]] || return 0

  local pair="" src="" dest_size="" icon_dir=""
  for pair in \
    "32x32.png|32x32" \
    "128x128.png|128x128" \
    "icon.png|256x256"; do
    src="${repo_icons}/${pair%%|*}"
    dest_size="${pair##*|}"
    [[ -f "$src" ]] || continue
    icon_dir="${target_home}/.local/share/icons/hicolor/${dest_size}/apps"
    mkdir -p "$icon_dir"
    install -m 644 -o "$target_user" -g "$target_user" "$src" \
      "${icon_dir}/com.backr.app.png"
  done

  if command -v gtk-update-icon-cache &>/dev/null; then
    local hicolor="${target_home}/.local/share/icons/hicolor"
    # External: gtk-update-icon-cache rebuilds the hicolor theme index (non-fatal).
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
  local dest="${dest_dir}/Backr.AppImage"
  local desktop_dir="${target_home}/.local/share/applications"
  local desktop="${desktop_dir}/com.backr.app.desktop"

  mkdir -p "$dest_dir" "$desktop_dir"
  install_host_backr_icon_to_user_theme "$target_user" "$target_home"

  # TryExec is intentionally omitted — some launchers hide entries whose TryExec binary
  # is missing. Omitting it means the entry always appears; clicking it before the
  # AppImage is downloaded will simply do nothing.
  cat >"$desktop" <<EOF
[Desktop Entry]
Version=1.5
Type=Application
Name=Backr (Host Dashboard)
GenericName=Backup host dashboard
Comment=Backr host-dashboard — inspect backups and trust client keys (rsync over SSH)
Exec=env BACKR_HOST_MODE=1 ${dest} %u
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

  # Always write the .desktop entry so the app appears in the launcher immediately.
  install_host_desktop_entry "$target_user" "$target_home"

  local appimage_src=""
  local built_src_dir=""

  if tmp="$(download_appimage_to_tempfile "$HOST_APPIMAGE_URL" 2>/dev/null)"; then
    # Pre-built release available — use it directly.
    appimage_src="$tmp"
    trap 'rm -f "$appimage_src"' RETURN
  else
    echo "Pre-built AppImage not available at ${HOST_APPIMAGE_URL} — falling back to source build …"
    # build_host_appimage_from_source prints progress and echoes the AppImage path on the last line.
    local build_out
    build_out="$(build_host_appimage_from_source "$target_user" "$target_home")" || {
      echo "warning: source build failed — the launcher entry is installed but the binary is missing." >&2
      echo "  Re-run this script or build manually: cd /path/to/Backr && npm ci && npm run tauri:build" >&2
      SKIP_HOST_APPIMAGE=1
      return 1
    }
    # Last line of build output is the AppImage path; the containing dir is the temp src dir.
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
  dest="${target_home}/.local/share/backr/Backr.AppImage"
  [[ -f "$dest" ]] || { echo "warning: AppImage not found at ${dest} — skipping auto-launch" >&2; return 0; }

  xdg_runtime="/run/user/${target_uid}"

  # Standard systemd D-Bus socket path — needed to spawn GUI apps from a root process.
  local dbus_addr="unix:path=${xdg_runtime}/bus"

  # Build the display environment: prefer Wayland (any wayland-* socket), fall back to X11.
  # Hyprland and some other compositors use wayland-1 rather than wayland-0.
  local display_args=()
  local wayland_sock=""
  for sock in "${xdg_runtime}"/wayland-*; do
    [[ -S "$sock" ]] && { wayland_sock="${sock##*/}"; break; }
  done

  if [[ -n "$wayland_sock" ]]; then
    display_args=(
      "WAYLAND_DISPLAY=${wayland_sock}"
      "XDG_RUNTIME_DIR=${xdg_runtime}"
      "DBUS_SESSION_BUS_ADDRESS=${dbus_addr}"
    )
  elif [[ -n "${DISPLAY:-}" ]]; then
    display_args=(
      "DISPLAY=${DISPLAY}"
      "XDG_RUNTIME_DIR=${xdg_runtime}"
      "DBUS_SESSION_BUS_ADDRESS=${dbus_addr}"
    )
  elif [[ -S "/tmp/.X11-unix/X0" ]]; then
    display_args=(
      "DISPLAY=:0"
      "XDG_RUNTIME_DIR=${xdg_runtime}"
      "DBUS_SESSION_BUS_ADDRESS=${dbus_addr}"
    )
  else
    echo "No graphical session detected for '${target_user}' — skipping auto-launch. Open Backr from the app menu when logged in."
    return 0
  fi

  echo "Launching Backr host dashboard for '${target_user}' …"
  # External: runuser runs the AppImage as the desktop user; & disowns it so the script exits cleanly.
  runuser -u "$target_user" -- env \
    "${display_args[@]}" \
    BACKR_HOST_MODE=1 \
    "$dest" &>/dev/null &
  disown $! 2>/dev/null || true
  echo "Backr host dashboard launched (it may take a moment to appear)."
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
    local du=""
    du="$(detect_desktop_user 2>/dev/null || echo '(desktop-user)')"
    printf '  Host dashboard: ~/.local/share/backr/Backr.AppImage (installed for %s)\n' "$du"
    printf '                  App is launching — use Trust keys (#/host/trust) to add laptop keys.\n'
  else
    printf '  Host dashboard: skipped (--no-appimage). Use authorized_keys to add client pubkeys.\n'
  fi
  cat <<EOF

On each laptop — clone the Backr repo and run:
  ./scripts/setup-connecting-client.sh
The wizard will ask for this machine's IP (${ip_line:-see Primary IP above}) and SSH port.
It handles deps, AppImage install, and key trust (ssh-copy-id) automatically.

EOF
}

main() {
  parse_args "$@"
  require_linux_root
  normalize_root
  run_backup_host_questionnaire

  install_server_ssh_rsync "$DRY_RUN"
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
  report_detected_ssh_environment
  print_host_ready
  emit_backup_host_custom_next_steps
}

main "$@"
