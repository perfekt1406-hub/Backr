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

On **Linux**, `./scripts/setup-connecting-client.sh` (default) installs the toolchain above, **`npm ci` / `npm install`**, runs **`npm run tauri:build`**, and installs the built **AppImage** plus a launcher entry under **`~/.local/share/`**. Pass **`--backup-host`** *hostname-or-IP* (or **`BACKR_BACKUP_HOST`**) so the script checks pubkey **`ssh`** (BatchMode; **no** passwords / **no** `ssh-copy-id`) and prints a **`BACKR_AUTHORIZED_KEYS`** one-liner if trust is still missing. Use **`--deps-only`** for toolchain + npm only (no build; for **`npm run tauri:dev`**).

The remote backup host must provide **`rsync`** and SSH access; **`setup-backup-host.sh`** merges **`BACKR_AUTHORIZED_KEYS`** / **`--pubkey`**, turns **`PubkeyAuthentication`** on, and applies **`Match User`** rules so the **`backr`** account is **pubkey-only** once **`authorized_keys`** has at least one key line (no SSH password for that account after that).

---

## Getting started

### Fast path (automated scripts only)

1. **On the backup machine (once, as root):**  
   `curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash`  
   To inject your laptop’s pubkey in the same step (no `ssh-copy-id` later), set **`BACKR_AUTHORIZED_KEYS`** as in [§5](#5-optional-linux-backup-host-script-nas--server).
2. **On your laptop:** clone the repo, then  
   `./scripts/setup-connecting-client.sh --backup-host BACKUP_IP_OR_DNS`  
   If pubkey login is not ready yet, the script prints your pubkey and the **`BACKR_AUTHORIZED_KEYS`** command — still **no** `ssh-copy-id` or SSH passwords in our scripts.
3. **Launch Backr** from the app menu and finish the in-app setup wizard.

Both scripts start an optional **multiple-choice questionnaire** when **`/dev/tty`** is available (arrow-key **`dialog`** / **`whiptail`** menus when possible — they **`apt`/`dnf`/… install `dialog`** if needed; the laptop script uses **`sudo`** for that step). Answers **`4`** = *I don't know*. At the end you get **tailored «what to do next»** text derived from those answers plus auto-detection. Piped installs (no TTY) or **`BACKR_NON_INTERACTIVE=1`** / **`--non-interactive`** skip prompts and print shorter defaults instead.

### 1. Clone

```bash
git clone https://github.com/perfekt1406-hub/Backr.git
cd Backr
```

### 2. Install Backr on your laptop (inside the repo)

**Linux (recommended):** one command installs Tauri deps, Node.js, Rust, **`npm ci` / `npm install`**, **`npm run tauri:build`**, and registers the **AppImage** in your app menu. Pass your backup server address so the script verifies pubkey **`ssh`** (defaults SSH user **`backr`** when you pass only a hostname or IP) and prints bootstrap commands if needed (**still no** `ssh-copy-id`):

```bash
./scripts/setup-connecting-client.sh --backup-host 192.168.1.50
```

Use **`--deps-only`** if you only want the dev toolchain (no release build or menu install).

```bash
./scripts/setup-connecting-client.sh --deps-only --backup-host nas.local
```

**Manual:** install the [requirements](#requirements) yourself, then:

```bash
npm install
```

### 3. Run in development

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

### 4. Production build

```bash
npm run tauri:build
```

Installable bundles are produced under `src-tauri/target/release/bundle/` according to your platform and `src-tauri/tauri.conf.json`.

### 5. Optional: Linux backup host script (NAS / server)

The backup helper detects **apt**, **dnf**, **yum**, **pacman**, **zypper**, **apk** and expects **sudo**. It installs **OpenSSH server** + **rsync**, validates **`sshd`**, ensures **`/etc/ssh/sshd_config.d/`** drop-ins apply, writes **`PubkeyAuthentication yes`**, creates **`backr`** + **`/srv/backr`**, merges optional pubkeys (**`BACKR_AUTHORIZED_KEYS`**, **`--pubkey`**, **`--pubkey-file`**). After **`authorized_keys`** contains at least one pubkey line for **`backr`**, it adds **`Match User`** rules so that account is **pubkey-only** (no SSH password / keyboard-interactive for **`backr`**). It fixes **SELinux** contexts when enforcing, opens **SSH** on **UFW** / **firewalld** only when those stacks are already active (never enables a firewall by surprise — use **`--no-firewall`** to skip), writes **`/etc/backr/host.toml`**, prints an **auto-detected** summary (**`/etc/os-release`**, firewall managers, **`sshd -T`** ports/auth toggles, **`ss`** listeners), and starts an optional **questionnaire** when **`/dev/tty`** exists (choice **4** = *I don't know*) followed by **tailored next-step instructions** (**--non-interactive** or unattended stdin pipes skip prompts).

**Typical one-liner on the backup machine** (no repo clone):

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash
```

**Trust your laptop key without running `ssh-copy-id` later** (run on the backup host; paste your pubkey line):

```bash
sudo BACKR_AUTHORIZED_KEYS="ssh-ed25519 AAAA… comment" bash -c 'curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | bash'
```

Repo checkout equivalent:

```bash
sudo ./scripts/setup-backup-host.sh --help
```

**Still environment-specific (not scripted):** router port-forwards, VPS/cloud security groups, proprietary NAS OS images without normal **`apt`/`dnf`**, SSH clients targeting the wrong **Port** vs **`sshd`**, and some **`yum`**-only images that omit WebKitGTK **4.1** for laptop builds (use Fedora/Ubuntu/Arch or [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux) manually).

---

## Configuration

On first launch, Backr sends you through setup if no config exists. Settings are persisted as TOML at:

- **Unix:** `~/.config/backr/config.toml`

Conceptually the file contains remote SSH targets, local `projects_path`, schedule `interval_hours`, and persisted `last_backup_at`. For a concrete schema and example values, see the inline documentation in [tauri-app-then-can-breezy-peacock.md](tauri-app-then-can-breezy-peacock.md) (implementation plan).

SSH host keys for backup connections are tracked in `~/.config/backr/known_hosts` (isolated from your default `known_hosts`).

Per-project snapshot counts and “last backup” labels on the **dashboard** are read from **`~/.config/backr/snapshot_stats.json`** by default (updated after each successful backup). When your laptop is away from the backup network, use **Sync from backup server** in the UI to refresh counts over SSH; browsing snapshot **contents** still requires connectivity.

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
| `scripts/` | `setup-backup-host.sh` & `setup-connecting-client.sh`: bootstrap + optional **questionnaires** (choice **4** = *I don't know*) → **tailored next steps**; **`--non-interactive`** skips prompts (pipes/CI); **no** `ssh-copy-id`. |

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
