# Backr

![Version](https://img.shields.io/badge/version-0.1.0-blue)
![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8D8?logo=tauri&logoColor=white)
![Svelte](https://img.shields.io/badge/Svelte-5-FF3E00?logo=svelte&logoColor=white)

Desktop app that backs up your projects folder to a remote machine over SSH — rsync snapshots, scheduled or on demand, with a UI for browsing and restoring past snapshots.

---

## Setup (two machines, ~5 minutes)

### 1. Backup host (NAS / server / spare PC) — run once as root

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-backup-host.sh | sudo bash
```

Installs OpenSSH + rsync, creates the `backr` account and `/srv/backr`, configures sshd, and opens the Backr host-dashboard app automatically. The app shows a step-by-step guide for connecting your first laptop.

Use `--no-appimage` on headless servers (no desktop session).

### 2. Laptop — one command (builds from source)

```bash
curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-connecting-client.sh | bash
```

Run as your **normal user** (not `sudo`) — it elevates per-command for package installs. It downloads the source, installs all build deps (Node, Rust, Tauri libs), builds a **native binary** (`tauri build --no-bundle`), and adds it to your app menu. Works on Debian/Ubuntu, Fedora, Arch-based, openSUSE, and Alpine.

Prefer a checkout? Clone and run the same script:

```bash
git clone https://github.com/perfekt1406-hub/Backr.git && cd Backr
./scripts/setup-connecting-client.sh
```

The wizard asks for the backup host's IP/hostname and SSH port. To trust this laptop's key on the host it offers `ssh-copy-id`, but the `backr` account is passwordless by default — so usually you'll paste your public key (`~/.ssh/id_ed25519.pub`) into Backr on the host → **Trust keys** (`#/host/trust`), or append it to `~backr/.ssh/authorized_keys`.

> **Re-running updates:** running either setup command again rebuilds from the latest source and replaces the installed app (stopping any running instance first).
>
> **Uninstall (laptop):** `./scripts/setup-connecting-client.sh --uninstall` removes the app, launcher entry, and icons (keeps your config, SSH keys, and toolchain).

### 3. Open Backr from the app menu and finish the in-app setup wizard.

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
