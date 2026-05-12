#!/usr/bin/env bash
#
# Purpose: Turn a generic Linux machine into a Backr backup target with minimal operator effort — packages, sshd,
#          firewall rules when a manager is already active, authorized_keys, backup tree, host-dashboard marker, optional
#          interactive questionnaire + tailored next-steps for facts scripts cannot infer (router/VPN/NAS uncertainty).
# Role: Distro-aware install of OpenSSH server + rsync; validates sshd_config; ensures drop-in snippets load;
#       optional pubkey ingestion (--pubkey, --pubkey-file, BACKR_AUTHORIZED_KEYS); Match blocks disable SSH passwords for
#       BACKR_USER only once authorized_keys holds keys; UFW/firewalld SSH allowance when already enforcing; SELinux
#       ~/.ssh contexts when enforcing; prints auto-detected OS/firewall/sshd facts (sshd -T, ss listeners);
#       optional questionnaire installs dialog when needed for arrow-key menus + tailored next-steps.
#
# Typical usage (on the backup machine, one command):
#   curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash
#
# Passwordless trust for your laptop’s pubkey (no ssh-copy-id; **backr** becomes pubkey-only after keys are installed):
#   sudo BACKR_AUTHORIZED_KEYS="$(cat /path/to/id_ed25519.pub)" bash -c 'curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | bash'
#
# Run with sudo on the machine that will receive rsync over SSH. Non-Linux hosts are not supported here.
#
# CLI options:
#   --user NAME          Dedicated account (default: backr).
#   --root PATH          Absolute backup root on disk (default: /srv/backr).
#   --pubkey LINE        SSH public key line (repeatable). Example: --pubkey "ssh-ed25519 AAAA... comment"
#   --pubkey-file PATH   Read key lines from file (repeatable).
#   --no-firewall        Do not add SSH allowances to UFW or firewalld even when active.
#   --non-interactive    Skip the questionnaire and abbreviated default next-steps (for pipes / CI).
#   --dry-run            Print actions only.
#   -h, --help           Show this text.
#
# Environment:
#   BACKR_AUTHORIZED_KEYS     Newline-separated pubkey lines to append (use with sudo -E when preserving env).
#   BACKR_NON_INTERACTIVE=1 Same as --non-interactive.

set -euo pipefail

BACKR_USER="${BACKR_USER:-backr}"
BACKR_ROOT="${BACKR_ROOT:-/srv/backr}"
DRY_RUN=0
SKIP_FIREWALL=0
# Populated by repeated --pubkey / --pubkey-file (inputs: CLI strings / paths; outputs: aggregated lines).
declare -a BACKR_CLI_PUBKEY_LINES=()
declare -a BACKR_CLI_PUBKEY_FILES=()
# Questionnaire / non-interactive (see run_backup_host_questionnaire / emit_backup_host_custom_next_steps).
BACKR_NON_INTERACTIVE="${BACKR_NON_INTERACTIVE:-0}"
SURVEY_SKIP_NO_TTY=0
SURVEY_DEPLOYMENT="${SURVEY_DEPLOYMENT:-unknown}"
SURVEY_REACH="${SURVEY_REACH:-unknown}"
SURVEY_SSH_PORT="${SURVEY_SSH_PORT:-unknown}"
SURVEY_SSH_CUSTOM_PORT="${SURVEY_SSH_CUSTOM_PORT:-}"
SURVEY_PLATFORM="${SURVEY_PLATFORM:-unknown}"
SURVEY_KEYPATH="${SURVEY_KEYPATH:-unknown}"

die() {
  echo "error: $*" >&2
  exit 1
}

usage() {
  sed -n '1,32p' "$0" | tail -n +2
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
      --pubkey)
        [[ -n "${2:-}" ]] || die "--pubkey needs a value"
        BACKR_CLI_PUBKEY_LINES+=("$2")
        shift 2
        ;;
      --pubkey-file)
        [[ -n "${2:-}" ]] || die "--pubkey-file needs a path"
        BACKR_CLI_PUBKEY_FILES+=("$2")
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
# Prints lines to the controlling terminal when available (stdin may be a pipe for curl | bash).
#
survey_print_tty() {
  printf '%s\n' "$@" >/dev/tty
}

#
# Inputs: none — must run as root. Outputs: installs dialog when missing on supported distros; no-op if dialog/whiptail exists.
# External: apt-get/dnf/yum/pacman/zypper/apk — aligned with install_server_ssh_rsync package families.
#
ensure_survey_tui_pkg_host() {
  command -v dialog &>/dev/null && return 0
  command -v whiptail &>/dev/null && return 0
  local backend
  backend="$(detect_pkg_backend)"
  export DEBIAN_FRONTEND=noninteractive
  case "$backend" in
    apt)
      apt-get update -qq
      apt-get install -y dialog || return 1
      ;;
    dnf)
      dnf install -y dialog || return 1
      ;;
    yum)
      yum install -y dialog || return 1
      ;;
    pacman)
      pacman -Sy --noconfirm dialog || return 1
      ;;
    zypper)
      zypper --non-interactive refresh
      zypper --non-interactive install -y dialog || return 1
      ;;
    apk)
      apk update
      apk add --no-cache dialog || return 1
      ;;
    *)
      return 1
      ;;
  esac
  command -v dialog &>/dev/null || command -v whiptail &>/dev/null
}

#
# Inputs: $1 question text, $2–$4 three option strings (fourth is always «I don't know»).
# Outputs: emits choice 1–4 on stdout (defaults to 4 when cancelled/invalid); prefers dialog then whiptail on /dev/tty.
#
survey_read_menu_4() {
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

  survey_print_tty ""
  survey_print_tty "$title"
  survey_print_tty "  1) $o1"
  survey_print_tty "  2) $o2"
  survey_print_tty "  3) $o3"
  survey_print_tty "  4) I don't know"
  survey_print_tty "Choice [1-4]:"
  read -r line </dev/tty 2>/dev/null || line=""
  line="$(printf '%s' "$line" | tr -d '[:space:]')"
  [[ "$line" =~ ^[1-4]$ ]] || line=4
  printf '%s' "$line"
}

#
# Runs an interactive questionnaire when /dev/tty exists and BACKR_NON_INTERACTIVE is unset.
# Outputs: fills SURVEY_* globals used by emit_backup_host_custom_next_steps.
#
run_backup_host_questionnaire() {
  [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]] && return 0
  [[ "$DRY_RUN" -eq 1 ]] && return 0
  [[ -c /dev/tty ]] || {
    SURVEY_SKIP_NO_TTY=1
    return 0
  }

  local c=""
  survey_print_tty ""
  survey_print_tty "=== Backr backup host — quick questionnaire ==="
  survey_print_tty "Choose the best match; option 4 means «I don't know» — you'll get discovery steps at the end."
  survey_print_tty ""
  ensure_survey_tui_pkg_host 2>/dev/null || true

  c="$(survey_read_menu_4 \
    "Where does this machine mainly run?" \
    "Home or office LAN" \
    "VPS or cloud VM" \
    "Other / mixed")"
  case "$c" in 1) SURVEY_DEPLOYMENT=lan ;; 2) SURVEY_DEPLOYMENT=cloud ;; 3) SURVEY_DEPLOYMENT=other ;; *) SURVEY_DEPLOYMENT=unknown ;; esac

  c="$(survey_read_menu_4 \
    "How will backup clients reach SSH on this box?" \
    "Same LAN only (private IPs)" \
    "Over the internet (public IP, DDNS, port forward)" \
    "VPN to this network first")"
  case "$c" in 1) SURVEY_REACH=lan_only ;; 2) SURVEY_REACH=internet ;; 3) SURVEY_REACH=vpn ;; *) SURVEY_REACH=unknown ;; esac

  c="$(survey_read_menu_4 \
    "Which SSH port does sshd use here?" \
    "Default 22" \
    "A different port (you'll type it next)" \
    "I'll verify using «Auto-detected» sshd Port lines after setup")"
  case "$c" in
    1)
      SURVEY_SSH_PORT=default
      SURVEY_SSH_CUSTOM_PORT=""
      ;;
    2)
      SURVEY_SSH_PORT=custom
      survey_print_tty "Enter TCP port number for sshd (e.g. 2222):"
      read -r SURVEY_SSH_CUSTOM_PORT </dev/tty 2>/dev/null || SURVEY_SSH_CUSTOM_PORT=""
      SURVEY_SSH_CUSTOM_PORT="${SURVEY_SSH_CUSTOM_PORT//[^0-9]/}"
      [[ -z "$SURVEY_SSH_CUSTOM_PORT" ]] && SURVEY_SSH_PORT=unknown
      ;;
    3 | 4)
      SURVEY_SSH_PORT=unknown
      SURVEY_SSH_CUSTOM_PORT=""
      ;;
    *)
      SURVEY_SSH_PORT=unknown
      SURVEY_SSH_CUSTOM_PORT=""
      ;;
  esac

  c="$(survey_read_menu_4 \
    "What best describes this OS?" \
    "Normal Linux server (apt/dnf/pacman-style packages)" \
    "NAS / appliance UI (Synology, QNAP, … or unclear Linux)" \
    "Unsure — I'll rely on script output / docs")"
  case "$c" in 1) SURVEY_PLATFORM=generic_linux ;; 2) SURVEY_PLATFORM=nas ;; *) SURVEY_PLATFORM=unknown ;; esac

  c="$(survey_read_menu_4 \
    "How will your laptop's SSH public key get into ${BACKR_USER}'s authorized_keys?" \
    "BACKR_AUTHORIZED_KEYS / --pubkey while running this script" \
    "I'll paste it later from a console or existing admin SSH session" \
    "Someone else administers this server")"
  case "$c" in 1) SURVEY_KEYPATH=inline ;; 2) SURVEY_KEYPATH=console_later ;; 3) SURVEY_KEYPATH=other_admin ;; *) SURVEY_KEYPATH=unknown ;; esac

  survey_print_tty ""
  survey_print_tty "Thanks — continuing setup…"
  survey_print_tty ""
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

  echo ""
  echo "── Your next steps (based on your questionnaire + this machine) ──"

  if [[ "${BACKR_NON_INTERACTIVE:-0}" == "1" ]]; then
    cat <<'NXT'
You used --non-interactive / BACKR_NON_INTERACTIVE — questionnaire was skipped.

  • If this was curl | sudo bash: run again from an interactive shell without --non-interactive if you want tailored hints.
  • Trust path (passwordless **backr**): supply BACKR_AUTHORIZED_KEYS or --pubkey on this host, or merge keys from console.
  • Clients must reach the sshd Port shown in «Auto-detected» above (same for LAN router/VPN/firewall rules).

NXT
    return 0
  fi

  if [[ "${SURVEY_SKIP_NO_TTY:-0}" == "1" ]]; then
    cat <<'NXT'
No controlling terminal (/dev/tty) — questionnaire was skipped (common with piped installs).

  • Re-run interactively from ssh/console: sudo bash scripts/setup-backup-host.sh
  • Or set BACKR_NON_INTERACTIVE=1 explicitly when automation must stay silent.

NXT
  fi

  if [[ "$SURVEY_DEPLOYMENT" == "unknown" ]]; then
    cat <<'NXT'
• You weren't sure where this machine «lives». Discover it:
    hostname -f ; hostname -I ; ip -br addr
  Decide whether backups will use a private LAN IP, a VPN address, or a public hostname — then use that same address from the laptop.

NXT
  fi

  if [[ "$SURVEY_REACH" == "unknown" ]]; then
    cat <<'NXT'
• You weren't sure how clients reach SSH. Check both paths:
    LAN: from another device on the same Wi‑Fi/Ethernet, ping this host's private IP and: nc -vz HOST 22 (or your SSH port).
    Internet: ensure your router forwards the SSH port to this machine and/or open the port in your cloud security group.
  If only VPN works, connect VPN first on the laptop before testing SSH.

NXT
  fi

  case "$SURVEY_REACH" in
    internet)
      cat <<'NXT'
• Internet exposure: confirm port-forward / cloud SG allows inbound TCP to the sshd port shown above; prefer key-only **backr** (already enforced once authorized_keys has keys).

NXT
      ;;
    vpn)
      cat <<'NXT'
• VPN path: document the VPN endpoint for laptops; SSH targets are usually private IPs visible only while VPN is up.

NXT
      ;;
    lan_only)
      cat <<'NXT'
• LAN-only: backups fail off-network — that is expected unless you add VPN or split routing.

NXT
      ;;
  esac

  if [[ "$SURVEY_SSH_PORT" == "unknown" ]]; then
    printf '%s\n' "• SSH port unclear — effective listen ports from sshd: ${eff_ports:-run «sshd -T | grep -i port» as root}"
    cat <<'NXT'
  From a client, try:  ssh -v -o ConnectTimeout=5 USER@HOST -p 22  then retry with other -p values if needed.

NXT
  fi

  if [[ "$SURVEY_SSH_PORT" == "custom" ]] && [[ -n "$SURVEY_SSH_CUSTOM_PORT" ]]; then
    printf '%s\n' "• You said SSH uses port ${SURVEY_SSH_CUSTOM_PORT}. Effective sshd ports here: ${eff_ports:-unknown}"
    cat <<'NXT'
  If they differ, edit /etc/ssh/sshd_config (or drop-ins), then systemctl reload sshd — or adjust your answer next run.

NXT
  fi

  case "$SURVEY_PLATFORM" in
    nas | unknown)
      cat <<'NXT'
• NAS / unknown OS: if this script's packages failed or sshd behaves oddly, check vendor docs for «SSH server» + rsync; you may need their package UI instead of apt/dnf.

NXT
      ;;
  esac

  case "$SURVEY_KEYPATH" in
    inline)
      if [[ -z "${BACKR_AUTHORIZED_KEYS:-}" ]] && [[ "${#BACKR_CLI_PUBKEY_LINES[@]}" -eq 0 ]] && [[ "${#BACKR_CLI_PUBKEY_FILES[@]}" -eq 0 ]]; then
        cat <<'NXT'
• You chose «inline keys» but didn't pass BACKR_AUTHORIZED_KEYS / --pubkey / --pubkey-file. Re-run with one of those, or paste your laptop pubkey into authorized_keys manually.

NXT
      fi
      ;;
    console_later | other_admin | unknown)
      cat <<'NXT'
• Pubkey install still manual on this host: append one line from the laptop's ~/.ssh/id_ed25519.pub to ~BACKR_USER/.ssh/authorized_keys (see README BACKR_AUTHORIZED_KEYS curl pattern).

NXT
      ;;
  esac

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
# Inputs: path to aggregate tempfile written by caller. Reads BACKR_CLI_* and BACKR_AUTHORIZED_KEYS env.
# Outputs: validated pubkey lines only (one per line) into $1.
#
build_pubkey_aggregate_file() {
  local out="$1"
  : >"$out"
  local f line
  for f in "${BACKR_CLI_PUBKEY_FILES[@]:-}"; do
    [[ -f "$f" ]] || die "pubkey file not found: $f"
    # External: cat copies file contents into the aggregate (inputs: file path; outputs: stdout lines).
    cat "$f" >>"$out"
  done
  for line in "${BACKR_CLI_PUBKEY_LINES[@]:-}"; do
    printf '%s\n' "$line" >>"$out"
  done
  if [[ -n "${BACKR_AUTHORIZED_KEYS:-}" ]]; then
    printf '%s\n' "${BACKR_AUTHORIZED_KEYS}" >>"$out"
  fi
}

#
# Inputs: none — reads pubkey aggregate via build_pubkey_aggregate_file.
# Outputs: merges unique valid pubkey lines into ~backup/.ssh/authorized_keys (inputs: existing keys + new lines).
#
merge_pubkeys_into_authorized_keys() {
  [[ "$DRY_RUN" -eq 1 ]] && echo "[dry-run] merge pubkey lines into ~${BACKR_USER}/.ssh/authorized_keys" && return 0
  local home_dir ak agg filtered tmp_out
  home_dir="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  [[ -n "$home_dir" ]] || die "could not resolve home for ${BACKR_USER}"
  ak="${home_dir}/.ssh/authorized_keys"

  agg="$(mktemp)"
  filtered="$(mktemp)"
  tmp_out="$(mktemp)"
  trap 'rm -f "$agg" "$filtered" "$tmp_out"' RETURN

  build_pubkey_aggregate_file "$agg"
  # Normalize: drop blanks/comments; keep only plausible pubkey lines.
  while IFS= read -r line || [[ -n "${line:-}" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" ]] && continue
    [[ "$line" =~ ^# ]] && continue
    if is_ssh_pubkey_line "$line"; then
      printf '%s\n' "$line" >>"$filtered"
    fi
  done <"$agg"

  if [[ ! -s "$filtered" ]]; then
    echo "No pubkey lines supplied — authorized_keys unchanged (use --pubkey, --pubkey-file, or BACKR_AUTHORIZED_KEYS)."
    return 0
  fi

  [[ -f "$ak" ]] && cp -a "$ak" "${ak}.bak-backr-$$"
  : >"$tmp_out"
  [[ -f "$ak" ]] && cat "$ak" >"$tmp_out"
  local k added=0
  while IFS= read -r k || [[ -n "${k:-}" ]]; do
    [[ -z "$k" ]] && continue
    if grep -Fxq "$k" "$tmp_out" 2>/dev/null; then
      continue
    fi
    printf '%s\n' "$k" >>"$tmp_out"
    added=$((added + 1))
    echo "Authorized key added for ${BACKR_USER} (${k:0:24}…)"
  done <"$filtered"

  if [[ "$added" -eq 0 ]]; then
    echo "All supplied keys were already present in authorized_keys."
    return 0
  fi

  run_cmd cp "$tmp_out" "$ak"
  run_cmd chown "${BACKR_USER}:${BACKR_USER}" "$ak"
  run_cmd chmod 600 "$ak"
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
  cat <<EOF

From your laptop (passwordless once pubkey is installed for ${BACKR_USER}):
  ssh ${BACKR_USER}@$(hostname -f 2>/dev/null || hostname 2>/dev/null || echo THIS_HOST)

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
  merge_pubkeys_into_authorized_keys
  sshd_write_backr_drop_in "$DRY_RUN"

  local home_for_selinux
  home_for_selinux="$(getent passwd "$BACKR_USER" | cut -d: -f6)"
  [[ -n "$home_for_selinux" ]] && selinux_restore_ssh_home_if_enforcing "$home_for_selinux"

  open_ssh_on_active_managed_firewalls
  write_host_marker
  verify_backup_host_ready
  report_detected_ssh_environment
  print_host_ready
  emit_backup_host_custom_next_steps
}

main "$@"
