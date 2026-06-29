# Backr

![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8D8?logo=tauri&logoColor=white)
![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)

Desktop app that backs up your projects folder to a remote machine over SSH — rsync snapshots, scheduled or on demand, with a UI for browsing and restoring past snapshots.

---

## Setup (two machines, ~5 minutes)

### 1. Backup host (NAS / server / spare PC)

Run the host setup script as root:

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash
```

Installs OpenSSH + rsync, creates the `backr` account and `/srv/backr`, configures sshd, and opens the Backr host dashboard. To connect a laptop, click **Trust keys → Add a laptop** — it shows a 6-digit pairing code.

**Flags:**

| Flag | Effect |
|------|--------|
| `--no-appimage` | Skip the Backr app download (headless/server installs with no desktop session). |
| `--remove-old-files` | Delete all existing snapshots under `/srv/backr` before setup — use this to clear broken or partial backups. |
| `--verbose` | Print OS/firewall/sshd diagnostics after setup. |
| `--dry-run` | Print actions without executing them. |

To pass flags through the pipe, add `-s --` after `bash` (without it, `bash` treats the flag as its own option):

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash -s -- --remove-old-files
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash -s -- --no-appimage --verbose
```

---

### 2. Laptop

Run the client setup script as your **normal user** (not `sudo`) — it elevates per-command for package installs:

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-connecting-client.sh | bash
```

Downloads the source, installs all build deps (Node, Rust, Tauri libs), builds a **native binary** (`tauri build --no-bundle`), and adds it to your app menu. Works on Debian/Ubuntu, Fedora, Arch-based, openSUSE, and Alpine.

To pass flags through the pipe, add `-s --` after `bash` (without it, `bash` treats the flag as its own option):

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-connecting-client.sh | bash -s -- --deps-only
```

Prefer a checkout?

```bash
git clone https://github.com/perfekt1406-hub/Backr.git && cd Backr
./scripts/setup-connecting-client.sh
```

On first launch Backr **scans the LAN for your host** (in pairing mode), you pick it and enter the **6-digit code**, and it generates your SSH key, gets it trusted on the host, pins the host key, and prefills the rest — no IP typing or key copying. If the host isn't found (some networks block mDNS), choose **Enter details manually** in the wizard.

> **Re-running reinstalls cleanly:** running the script again wipes the previous binary and config so the app opens in pairing mode, then rebuilds from the latest source.
>
> **Uninstall (laptop):** `./scripts/setup-connecting-client.sh --uninstall` removes the app, launcher entry, and icons (keeps your SSH keys and toolchain).

---

### 3. Open Backr on the laptop — it finds the host, you enter the code, done.

---

## Development

**Install deps only** (no build):

```bash
./scripts/setup-connecting-client.sh --deps-only
# or manually:
npm install
```

**Run dev server:**

```bash
npm run tauri:dev          # full desktop app
npm run tauri:dev:mock     # mocked IPC backend (no SSH/rsync needed)
npm run dev                # browser only
```

**Build:**

```bash
npm run tauri:build
```

Bundles land in `src-tauri/target/release/bundle/`.

---

## How it works

- Each subdirectory of your configured `projects_path` is a separate project.
- Backups use `rsync --link-dest` so unchanged files are hardlinked — multiple snapshots without duplicating data.
- The backup host only needs SSH + rsync. The Backr app is optional on the host (for the Trust keys UI).
- `backr` becomes pubkey-only automatically once `authorized_keys` holds at least one key.
- Config lives at `~/.config/backr/config.toml`; SSH known hosts at `~/.config/backr/known_hosts`.

---

## Project layout

| Path | Role |
|------|------|
| `src/` | Svelte 5 UI — routes, stores, components. |
| `src-tauri/` | Rust — config, scheduler, rsync/SSH, Tauri commands. |
| `scripts/` | `setup-backup-host.sh` and `setup-connecting-client.sh`. |

---

## License

[MIT](LICENSE)
