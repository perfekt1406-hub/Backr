# Settings Screen — Requirements

**Created:** 2026-06-23  
**Status:** Ready for planning

---

## Problem

There is no way to change anything after first-run without re-entering the setup wizard from the beginning. The sidebar "Settings" item in client mode currently points back to that wizard — a friction-heavy re-entry that assumes you want to redo everything. The host has no settings at all.

## Goals

- Let users edit any piece of their backup config without re-running the wizard.
- Give the host a lightweight management screen for trusted keys and host info.
- Make the setup wizard first-run only; settings owns day-2 edits.

## Actors

- **Client user** — the laptop owner; most likely to change connection details or project list.
- **Host user** — managing which laptops are trusted; rarely changes anything else.

---

## Client Settings (`/settings`)

Replaces the current sidebar "Settings → wizard" re-entry. Single scrollable page, three sections.

### Connection

Editable fields for:
- Hostname / IP
- Port (default 22)
- SSH user
- SSH key path (file picker or text input)
- Remote backup root path
- Known-hosts file path

Includes a **Test connection** button (same SSH probe as the wizard's Verify step). Saves to the existing `Config.remote` shape.

### Projects

Displays the current list of local folder paths. User can:
- Add a path (file picker or paste)
- Remove a path (per-row remove button)

Maps to `Config.paths.local_projects`.

### Schedule

Single field: backup interval in hours. Maps to `Config.schedule.interval_hours`.

---

## Host Settings (`/host/settings`)

New screen; needs a new sidebar item ("Settings") in host mode. Single scrollable page, two sections.

### Host Info

Read-only display of:
- Backup root path
- SSH user
- `authorized_keys` file path

These are set by the install script; the UI shows them for reference, not editing.

### Trusted Keys

Lists every pubkey currently in `authorized_keys` — one row per key, showing the key type and comment (the `user@machine` label at the end of each line).

Each row has a **Remove** button. Removing a key writes the updated file immediately (no pending state).

Also includes:
- **Add key** — the same paste-a-pubkey-line form that lives in the first-run guide's manual trust zone.
- **Start pairing** — the same one-tap pairing panel from the guide, embedded inline (no card wrapper).

---

## New IPC Commands Required

Two new Rust commands are needed for the trusted-key list:

| Command | Input | Output |
|---|---|---|
| `host_list_authorized_pubkeys` | — | `Vec<{ key_type, key_b64, comment, raw_line }>` |
| `host_remove_authorized_pubkey` | `raw_line: String` | `{ removed: bool, pubkey_line_count: u32 }` |

Both operate on the `authorized_keys` file for the `backr` UNIX account (same path as `host_trust_status` already resolves).

---

## Navigation

| Mode | Sidebar item | Route |
|---|---|---|
| Client | Settings (already exists, currently → wizard) | `/settings` |
| Host | Settings (new) | `/host/settings` |
| Setup | — | No settings until setup completes |

The `/setup` wizard route stays unchanged — it remains the first-run flow only.

---

## Scope Boundaries

**In scope:**
- Client: connection, projects, schedule
- Host: read-only host info, trusted key list + per-key revoke + add key + pairing

**Deferred:**
- Per-project backup options (exclude patterns, rsync flags)
- Multiple host profiles / connection switching
- SSH key generation from within settings (currently done by the install script)

**Out of scope:**
- App-level preferences (theme, notifications — no prefs system exists yet)
- Advanced rsync configuration

---

## Success Criteria

- A user can change their host IP (or any other connection field) without touching the wizard.
- A user can add or remove a project folder without touching the wizard.
- A host user can see which laptops are trusted and remove one by name.
- Test connection works from settings identically to the wizard's verify step.
- The wizard remains unchanged and still works for fresh installs.
