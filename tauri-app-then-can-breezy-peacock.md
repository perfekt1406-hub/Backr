# Backr — Implementation Plan

## Context

Backr is a Tauri + Svelte 5 desktop app that backs up `~/Projects/` to a remote machine on the local network using rsync hardlink snapshots. Each subfolder of `~/Projects/` is treated as a separate project. Backups run automatically every 3 hours and on demand. The UI lets the user browse any snapshot's file tree and restore a full snapshot to a new local folder (`~/Projects-<timestamp>`). No branches, no staging, no diffs — just snapshots.

---

## Stack

- **Tauri 2** (Rust backend + WebKitGTK renderer on Linux)
- **Svelte 5 + Vite + TypeScript** (frontend, runes-based reactivity)
- **Svelte stores** (shared state across components)
- **svelte-spa-router** (hash-based routing — works out of the box in Tauri)
- **Tailwind CSS 4** (via `@tailwindcss/vite`, no config file needed)
- **lucide-svelte**, **date-fns** (relative timestamps)
- rsync + SSH via `tokio::process::Command` (no C SSH library dependency)

Bootstrap command:
```
npm create tauri-app@latest backr -- --template svelte-ts --manager npm
```

---

## Remote Snapshot Layout

```
/backups/                          ← remote_path in config
  my-app/
    2026-05-10_09-00-00/           ← hardlink snapshot (full tree, minimal disk)
    2026-05-10_12-00-00/
  other-project/
    ...
```

---

## Config Schema (`~/.config/backr/config.toml`)

```toml
[remote]
host        = "192.168.1.50"
user        = "pi"
ssh_key     = "/home/alice/.ssh/id_ed25519"
backup_path = "/backups"

[local]
projects_path = "/home/alice/Projects"

[schedule]
interval_hours = 3

[state]
last_backup_at = "2026-05-10T09:00:00Z"   # written by app after each run
```

---

## Rust Project Structure (`src-tauri/src/`)

```
main.rs              ← Tauri builder, register commands, setup tray + scheduler
lib.rs               ← re-exports, AppState declaration
error.rs             ← BackrError (thiserror), impl Into<String>
state.rs             ← AppState: Mutex<Config>, AtomicBool in_progress, scheduler handles
config.rs            ← Config structs (serde), load_config(), save_config(), config_path()
scheduler.rs         ← tokio loop with CancellationToken, restartable on config change
tray.rs              ← TrayIconBuilder, menu items, tooltip updates
commands/
  mod.rs
  config_cmd.rs      ← get_config, save_config, test_connection
  project_cmd.rs     ← list_projects, get_backup_status
  backup_cmd.rs      ← run_backup (fire-and-forget, emits backup://progress events)
  snapshot_cmd.rs    ← list_snapshots, list_files, restore_snapshot
backup/
  mod.rs
  rsync.rs           ← build rsync argv, spawn, stream stdout lines as events
  ssh.rs             ← ssh ping (test_connection), ssh find for list_files
```

### Key `Cargo.toml` dependencies
```toml
tauri            = { version = "2.11", features = ["tray-icon", "image-png"] }
tauri-plugin-single-instance = "2.4"
tauri-plugin-notification    = "2.3"
tauri-plugin-shell           = "2.3"
tauri-plugin-dialog          = "2.7"
tokio    = { version = "1.52", features = ["full"] }
serde    = { version = "1.0", features = ["derive"] }
toml     = "1.1"
chrono   = { version = "0.4", features = ["serde"] }
thiserror = "2.0"
tracing   = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

### Key `package.json` dependencies
```json
{
  "dependencies": {
    "svelte": "^5.0.0",
    "svelte-spa-router": "^4.0.0",
    "@tauri-apps/api": "^2.11.0",
    "@tauri-apps/plugin-shell": "^2.3.0",
    "@tauri-apps/plugin-dialog": "^2.7.0",
    "@tauri-apps/plugin-notification": "^2.3.0",
    "lucide-svelte": "^0.511.0",
    "date-fns": "^4.1.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.1",
    "@sveltejs/vite-plugin-svelte": "^5.0.0",
    "vite": "^6.3.0",
    "svelte-check": "^4.0.0",
    "typescript": "^5.8.0",
    "tailwindcss": "^4.3.0",
    "@tailwindcss/vite": "^4.3.0"
  }
}
```

---

## Tauri Commands

All return `Result<T, String>` — errors are user-facing strings.

| Command | Signature | Notes |
|---|---|---|
| `get_config` | `() → Option<Config>` | null if unconfigured |
| `save_config` | `(Config) → ()` | writes TOML, restarts scheduler |
| `test_connection` | `(host, user, key_path) → ()` | ssh echo test |
| `list_projects` | `() → Vec<ProjectInfo>` | reads local subfolders + last backup time |
| `get_backup_status` | `() → BackupStatus` | last/next run, in_progress flag |
| `run_backup` | `(project?: String) → ()` | fire-and-forget; emits `backup://progress` |
| `list_snapshots` | `(project) → Vec<SnapshotEntry>` | ssh ls, sorted newest-first |
| `list_files` | `(project, snapshot, path) → Vec<FileEntry>` | ssh find -maxdepth 1 |
| `restore_snapshot` | `(project, snapshot) → String` | returns local restore path |

### AppState
```rust
pub struct AppState {
    pub config:           Mutex<Option<Config>>,
    pub in_progress:      AtomicBool,
    pub active_project:   Mutex<Option<String>>,
    pub last_backup_at:   Mutex<Option<DateTime<Utc>>>,
    pub scheduler_handle: Mutex<Option<JoinHandle<()>>>,
    pub scheduler_cancel: Mutex<Option<CancellationToken>>,
}
```

---

## Rsync Commands

### Backup (hardlink snapshot)
```
rsync --archive --hard-links --delete --info=progress2 --human-readable
      --rsh "ssh -i <key> -o StrictHostKeyChecking=accept-new -o BatchMode=yes
                          -o UserKnownHostsFile=~/.config/backr/known_hosts"
      [--link-dest <backup_path>/<project>/<prev_snapshot>]   # omit on first backup
      <projects_path>/<project>/                              # trailing slash = contents
      <user>@<host>:<backup_path>/<project>/<new_snapshot>/
```

### Restore
```
rsync --archive --info=progress2 --human-readable
      --rsh "ssh -i <key> -o BatchMode=yes"
      <user>@<host>:<backup_path>/<project>/<snapshot>/
      ~/Projects-<snapshot>/
```

---

## Svelte Frontend Structure (`src/`)

```
main.ts              ← mount App, import global CSS
App.svelte           ← onMount: get_config → push('/setup') if null
                       registers backup://progress listener once (unlisten on destroy)
routes.ts            ← svelte-spa-router route map
stores/
  config.ts          ← writable<Config | null>, load(), save(), testConn()
  projects.ts        ← writable<ProjectInfo[]>, refresh()
  backup.ts          ← writable<BackupStatus>, progressLog writable<string[]>, runBackup()
  snapshots.ts       ← writable Maps: project→snapshots, cacheKey→files
lib/
  commands.ts        ← typed invoke() wrappers for all Tauri commands
  events.ts          ← listen('backup://progress', ...) setup/teardown helper
  time.ts            ← date-fns formatDistanceToNow wrapper
components/
  layout/  AppShell.svelte, SidebarNav.svelte
  setup/   SetupWizard.svelte, StepRemote.svelte, StepPaths.svelte, StepVerify.svelte
  dashboard/ DashboardView.svelte, ProjectListItem.svelte, BackupNowButton.svelte
  project/   ProjectView.svelte, SnapshotTimeline.svelte, SnapshotItem.svelte
  snapshot/  SnapshotBrowserView.svelte, FileTree.svelte, FileTreeNode.svelte (recursive, lazy)
  shared/    StatusBadge.svelte, ProgressBar.svelte, ErrorToast.svelte, ConfirmDialog.svelte
types/   config.ts, project.ts, backup.ts, snapshot.ts
app.css              ← @tailwind directives
```

### Routes (`routes.ts` — svelte-spa-router)
```ts
export default {
  '/setup':                   SetupWizard,
  '/':                        DashboardView,
  '/project/:name':           ProjectView,
  '/project/:name/:snapshot': SnapshotBrowserView,
}
```
`App.svelte` renders `<Router {routes} />`. On mount, if config is null, `push('/setup')`.

### State management
Svelte's built-in `writable` stores replace Pinia — each store file exports a store plus async actions calling `lib/commands.ts`. Components subscribe with the `$store` shorthand. Svelte 5 runes (`$state`, `$derived`) are used inside components for local reactive state.

---

## Screens

1. **Setup Wizard** — 3 steps: remote creds → local/remote paths → test connection + confirm. Saves `config.toml` on finish.
2. **Dashboard** — list all projects (name, last backup time, status). "Back Up Now" for all projects; show backup progress while a run is active.
3. **Project View** — timeline list of snapshots newest-first, each with timestamp + "Restore" button (confirm dialog before triggering).
4. **Snapshot Browser** — lazy-loading file tree. Folders expand on click, fetching children via `list_files`. No file preview.

---

## System Tray

- Icon with tooltip: `"Backr — last backup: 2 hours ago"`
- Menu: **Open Backr** | **Back Up Now** | --- | **Quit**
- Window close button hides (not quits) the window — handled in `App.svelte` via `onCloseRequested`

---

## Edge Cases to Handle

| Issue | Handling |
|---|---|
| First backup (no prev snapshot) | SSH ls non-zero exit → omit `--link-dest` |
| Concurrent backup trigger | `AtomicBool.compare_exchange` guard; return error to user |
| Restore destination exists | Check before rsync; append `-1`, `-2` suffix |
| Non-matching snapshot dir names | Regex filter in `list_snapshots`; skip silently |
| SSH known_hosts | Use `~/.config/backr/known_hosts` (isolated from user's) |
| `--link-dest` must be absolute | Always construct full absolute remote path |
| Paths with spaces | Use `Command::arg()` not shell strings (no shell interpolation) |
| `ls` parsing fragility | Use `find -maxdepth 1 -printf '%y\t%s\t%T@\t%f\n'` instead |

---

## Implementation Order

1. Scaffold with `npm create tauri-app` + set `Cargo.toml` / `package.json` deps
2. `config.rs` + `get_config`/`save_config` commands
3. SetupWizard frontend (3 steps) wired to `save_config` + `test_connection`
4. `backup/rsync.rs` + `run_backup` command with event emission
5. `list_projects` + DashboardView
6. Scheduler (`scheduler.rs`) + system tray (`tray.rs`)
7. `list_snapshots` / `list_files` / `restore_snapshot` + ProjectView + SnapshotBrowserView
8. Wire backup event listener in `App.svelte` for live progress everywhere
9. Window-close-to-hide + single-instance plugin
10. Error handling, loading states, tray tooltip updates

---

## Verification

- `cargo tauri dev` launches the window; first launch shows wizard
- Fill wizard with real remote creds → "Test connection" returns success
- "Back Up Now" → rsync runs, progress lines appear in UI, snapshot appears under project
- Open project → snapshot listed → open snapshot → file tree matches remote
- Click "Restore" → `~/Projects-<timestamp>` folder created with correct contents
- Close window → app stays in tray; reopen via tray "Open Backr"
- Wait 3 hours (or temporarily set `interval_hours = 0.001` for a quick test) → auto backup fires
