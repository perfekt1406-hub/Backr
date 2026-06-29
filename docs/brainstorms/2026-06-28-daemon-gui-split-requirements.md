---
title: "Daemon / GUI Split"
created: 2026-06-28
status: draft
---

# Daemon / GUI Split — Requirements

## Problem

The Backr scheduler, SSH keep-alive, rsync jobs, and system tray all live inside the single Tauri
process that owns the window. This forces a `prevent_close` / `window.hide()` workaround to keep
the scheduler running after the user "closes" the app. The window cannot relaunch cleanly because
the process never actually exited. Config changes on disk (e.g. a reinstall that clears
`~/.config/backr/`) are invisible until the process is killed and restarted.

## Goal

Make `backrd` the real Backr — a persistent daemon that owns scheduling, rsync, SSH, the system
tray, and an IPC server. The Tauri GUI and a new CLI become thin frontends that connect to a
running daemon. The window can open and close freely without affecting any background operation.

## Scope

Applies to **both machines**:

- **Client** (laptop): daemon runs the backup scheduler and owns the client tray.
- **Host** (NAS / server): daemon runs the trust listener, disk monitor, and owns the host tray.

Both sides share the same daemon architecture and the same JSON IPC protocol.

## Actors

- **User** — opens the GUI or runs CLI commands when they want to view or change something.
- **`backrd`** — always-running daemon; the authoritative source of Backr state.
- **`backr` GUI** — Tauri window; optional, opens on demand, connects to the local daemon.
- **`backr` CLI** — terminal interface; sends commands to `backrd` and prints output.

## Key Behaviors

### Daemon (`backrd`)

- Starts at user login via a **systemd user service** (Linux) or **launchd user agent** (macOS),
  registered by the install script.
- On the client: runs the backup scheduler, fires rsync jobs, updates the tray tooltip with last
  backup time. Tray menu: Open Backr, Back Up Now, Status, Quit.
- On the host: serves the trust/pairing listener, monitors disk usage, updates the host tray.
- Listens on a **Unix domain socket** (`~/.local/share/backr/backrd.sock`) for JSON IPC from the
  GUI and CLI.
- Handles the full pairing protocol (mDNS, HTTP handshake, key exchange) — the GUI and CLI are
  just frontends to those operations.
- Starts in an **unconfigured idle state** if no config exists; tray shows "Not configured — open
  Backr to pair." Scheduler activates once a valid config is received over IPC.
- Emits progress events (backup started, file count, errors, completion) over the socket so the
  GUI can stream them when open.

### GUI (`backr` Tauri app)

- Opens on demand (tray click, launcher, `backr gui`). Connects to `backrd.sock` on launch.
- All Tauri commands that previously ran logic directly now proxy to the daemon over IPC.
- If the daemon is not running, the GUI attempts to start it once, then shows a clear error if
  it still can't connect.
- Window can be **truly closed** (no hide-on-close). Relaunching opens a fresh window that
  reconnects to the still-running daemon.
- Re-evaluates its mode (setup / client / host) on each launch by querying the daemon's current
  state — handles the "config was cleared while daemon was running" case cleanly.

### CLI (`backr`)

- Connects to `backrd.sock`, sends a JSON command, prints the result, exits.
- Core commands: `backr backup [project]`, `backr status`, `backr list [project]`,
  `backr config get/set <key> <value>`, `backr pair`, `backr snapshots [project]`.
- Host commands: `backr trust add <pubkey>`, `backr trust list`, `backr trust remove <id>`.
- Non-interactive output (suitable for scripts); human-readable by default, `--json` for
  machine-readable.

### Pairing

- Pairing logic lives in `backrd`. GUI and CLI call the same IPC commands:
  `discover_hosts`, `pair_with_host`, `confirm_pairing` (client side);
  `start_pairing`, `stop_pairing`, `pairing_status` (host side).
- Host pairing code appears in: tray menu, CLI output (`backr pair --host`), and GUI if open.
- Client side: `backr pair` runs an interactive terminal wizard (mDNS scan → pick host → enter
  code → confirm fingerprint); GUI wizard works the same way via IPC.

### Install scripts

- `setup-connecting-client.sh` registers `backrd` as a **systemd user service** on Linux
  (`~/.config/systemd/user/backrd.service`) and enables + starts it.
- `setup-backup-host.sh` does the same on the host.
- macOS: both scripts create a **launchd user agent** plist under
  `~/Library/LaunchAgents/com.backr.daemon.plist`.
- Uninstall: `backr uninstall` (or `--uninstall` flag) stops and removes the service unit,
  socket, and binaries. Config and SSH keys are kept unless `--purge` is passed.

## Scope Boundaries

**In scope:**
- Separate `backrd` daemon binary (no WebView, lean Rust + Tokio + tray)
- Shared core library crate for config, SSH, rsync, scheduler, pairing logic
- JSON Unix socket IPC protocol
- CLI (`backr`) with the commands listed above
- systemd / launchd service registration in install scripts
- GUI proxying all commands through IPC instead of running logic directly

**Deferred for later:**
- Windows support (no Unix sockets; would need named pipes — leave for a future pass)
- Web UI served by `backrd` (the GUI is sufficient for now)
- Remote CLI (connecting to a daemon on another machine over SSH)
- Daemon auto-update

**Outside scope:**
- Changing the backup protocol (rsync / SSH stays as-is)
- Rewriting the Svelte frontend (same UI, just talking to daemon via IPC instead of Tauri invoke)
- Multi-user daemon (daemon always runs as the login user)

## Success Criteria

- Closing the GUI window truly exits the Tauri process; `backrd` keeps running and fires
  scheduled backups.
- Opening the GUI after it was closed reconnects to the live daemon; no re-bootstrap of
  scheduler or SSH needed.
- `backr backup` from the terminal triggers an rsync job and streams progress lines.
- `backr status` shows last backup time, next scheduled time, and whether a job is in progress.
- A fresh install (no config) starts `backrd` successfully; tray shows "Not configured"; `backr
  pair` or the GUI wizard completes pairing and the daemon activates the scheduler without restart.
- Clearing `~/.config/backr/` while `backrd` is running and then opening the GUI shows the setup
  wizard (not stale client mode).

## Dependencies / Assumptions

- Linux (primary): systemd user sessions with `XDG_RUNTIME_DIR` available. Verified against
  `setup-connecting-client.sh` target distros.
- macOS: launchd user agents. Not yet tested — implementation should gate on `#[cfg(target_os)]`.
- `tiny_http` is already in `Cargo.toml` (used for the ephemeral pairing server) — reusable
  pattern for reference, though the IPC socket replaces it for daemon communication.
- `tokio` 1.52 already present; the daemon reuses the async runtime.
- Tray support on Linux requires a system tray indicator (libappindicator or StatusNotifierItem).
  Current Tauri tray already handles this; the daemon inherits the same requirement.

## Outstanding Questions

- **IPC schema**: Define the full JSON message format (request/response envelope, event push
  shape, error codes). Defer to planning — implementation can derive from existing Tauri command
  signatures.
- **Socket permissions**: The socket must be user-owned and mode 0600. Verify this is enforced
  on both Linux and macOS.
- **Daemon upgrade path**: When `backrd` is replaced by a new binary (install script re-run),
  the running daemon should be stopped first. Needs a clean stop sequence.
- **GUI fallback**: If `backrd.sock` doesn't exist and the GUI can't start the daemon, should the
  GUI fall back to running embedded logic (current behavior) or fail hard? Failing hard is simpler
  and forces correct setup; falling back preserves a degraded experience.
