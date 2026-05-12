# Backr

![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8D8?logo=tauri&logoColor=white)
![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)

**Backr** is a desktop application that backs up a local projects folder to a remote machine over SSH using **rsync** and **hardlink-based snapshots**. Each immediate subdirectory of your projects path is treated as its own project. You can run backups on a schedule or on demand, browse past snapshots in the UI, and restore copies of files or entire snapshots.

---

## Why Backr?

- **Snapshot-style backups** — Remote layout uses timestamped directories so you keep multiple points in time without duplicating unchanged files (hardlinks).
- **Simple mental model** — No branches or staging: one rolling backup stream per project with a clear timeline.
- **Local-first desktop UX** — Built with [Tauri 2](https://v2.tauri.app/) (Rust backend, native webview) and [Svelte 5](https://svelte.dev/) for a fast, focused UI.
- **Tray and scheduler** — Automatic runs on an interval you configure, plus manual backup from the app; progress streams into the shell.
- **Browse and restore** — Inspect snapshot file trees and restore into a new local folder (for example under `~/Projects-<timestamp>`).

---

## Requirements

Before you build or run Backr, install:

| Tool | Notes |
|------|--------|
| **Node.js** (npm) | For the Vite/Svelte frontend and npm scripts. |
| **Rust** | `rust-version` in `src-tauri/Cargo.toml` is **1.77.2** or newer. Use `rustup` to install a matching toolchain. |
| **System libraries for Tauri** | Follow the [official Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS (on Linux this typically includes WebKitGTK and related packages). |
| **`ssh` and `rsync`** | Used at runtime for backups and remote listing; required on the machine running Backr. |

The remote backup host must accept SSH public-key authentication and provide `rsync` on the server side.

---

## Getting started

### 1. Clone and install dependencies

```bash
git clone https://github.com/perfekt1406-hub/Backr.git
cd backr
npm install
```

### 2. Run in development

**Full desktop app** (starts Vite on port `1420` and opens the Tauri window):

```bash
npm run tauri:dev
```

**Frontend only** (browser — limited; Tauri `invoke` calls will not work unless you use mock mode):

```bash
npm run dev
```

**Desktop app with a mocked backend** (useful for UI work without SSH/rsync):

```bash
npm run tauri:dev:mock
# or browser:
npm run dev:mock
```

Mock mode is enabled when the environment variable `VITE_BACKR_MOCK` is set to `1`.

### 3. Production build

```bash
npm run tauri:build
```

Installable bundles are produced under `src-tauri/target/release/bundle/` according to your platform and `src-tauri/tauri.conf.json`.

### 4. Optional: prepare machines with helper scripts

On a **fresh Linux install**, these scripts detect **apt**, **dnf**, **yum** (Amazon Linux 2), **pacman**, **zypper**, or **apk** and install missing **OpenSSH** / **rsync** packages (the backup-host script also enables **sshd** and drops in **PubkeyAuthentication yes**). You still need **sudo** on the client when packages are missing.

On the **backup server** (Linux):

```bash
sudo ./scripts/setup-backup-host.sh --help
```

On the **machine that runs Backr** (Linux desktop):

```bash
./scripts/setup-connecting-client.sh --help
```

---

## Configuration

On first launch, Backr sends you through setup if no config exists. Settings are persisted as TOML at:

- **Unix:** `~/.config/backr/config.toml`

Conceptually the file contains remote SSH targets, local `projects_path`, schedule `interval_hours`, and persisted `last_backup_at`. For a concrete schema and example values, see the inline documentation in [tauri-app-then-can-breezy-peacock.md](tauri-app-then-can-breezy-peacock.md) (implementation plan).

SSH host keys for backup connections are tracked in `~/.config/backr/known_hosts` (isolated from your default `known_hosts`).

---

## Useful npm scripts

| Script | Purpose |
|--------|---------|
| `npm run dev` | Vite dev server on `http://localhost:1420`. |
| `npm run dev:mock` | Same with `VITE_BACKR_MOCK=1`. |
| `npm run build` | Production frontend build to `dist/` (also run before `tauri build`). |
| `npm run preview` | Preview the built SPA. |
| `npm run check` | `svelte-check` TypeScript/Svelte diagnostics. |
| `npm run tauri:dev` | Tauri development window. |
| `npm run tauri:dev:mock` | Tauri dev with mocked IPC backend. |
| `npm run tauri:build` | Ship-ready desktop bundles. |

---

## Project layout (high level)

| Path | Role |
|------|------|
| `src/` | Svelte 5 UI, routes, stores, and TypeScript command wrappers. |
| `src-tauri/` | Rust crate: config, scheduler, tray, rsync/SSH backup, Tauri commands. |
| `scripts/` | Host/client setup scripts plus `scripts/lib/` (distro-aware package helpers). |

UI design notes can live in `brand-aesthetic.md` locally (that filename is gitignored). A deeper technical plan and remote snapshot layout are documented in [tauri-app-then-can-breezy-peacock.md](tauri-app-then-can-breezy-peacock.md).

---

## Getting help

- **Tauri:** [Tauri 2 documentation](https://v2.tauri.app/)
- **Svelte / Vite:** [Svelte](https://svelte.dev/docs/svelte/overview), [Vite](https://vite.dev/guide/)
- **This repo:** Use the implementation plan and brand docs linked above for architecture and UX intent.

If the project is hosted on GitHub or another forge, use **Issues** for bugs and feature discussion once those are enabled.

---

## Maintainers and contributing

Backr is authored by **Backr contributors** (see `authors` in `src-tauri/Cargo.toml`). Community contributions are welcome: fork the repository, work on a focused branch, and open a pull request with a clear description of behavior changes.

---

## License

Backr is released under the [MIT License](LICENSE). Third-party dependencies (npm packages and Rust crates) remain under their respective licenses.
