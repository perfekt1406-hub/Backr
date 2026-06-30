/*
 * backr-core — portable business logic shared across Backr runtimes.
 *
 * This crate contains all modules that have no dependency on Tauri or any
 * desktop-GUI framework. It can be compiled into a daemon, a CLI tool, or
 * the Tauri GUI without modification.
 *
 * Module layout:
 *
 *  backup/          — rsync orchestration, SSH helpers, excludes, path validation.
 *  pairing/         — mDNS discovery, pairing code session, client & host listener.
 *  config           — Config struct and disk load/save helpers.
 *  error            — BackrError (internal) and BackrCommandError (IPC-serializable).
 *  host_config      — Host-dashboard marker detection (reads /etc/backr/host.toml).
 *  host_disk_inventory — Disk usage inventory for the backup-host dashboard.
 *  host_trust       — authorized_keys management for the Trust-keys UI.
 *  ipc_protocol     — Shared NDJSON IPC wire types (daemon⇄GUI single source of truth).
 *  progress_sink    — ProgressSink trait + CollectLines test helper.
 *  project_snapshot_cache — Per-project remote snapshot stats local cache.
 *  scheduler        — Periodic backup scheduler with BackupTrigger abstraction.
 */

pub mod backup;
pub mod config;
pub mod error;
pub mod host_config;
pub mod host_disk_inventory;
pub mod host_trust;
pub mod ipc_protocol;
pub mod pairing;
pub mod progress_sink;
pub mod project_snapshot_cache;
pub mod scheduler;

/// Returns the shared workspace version embedded in this binary at build time.
///
/// All three binaries (`backrd`, `backr-app`, `backr`) link this crate and
/// inherit the single `[workspace.package] version`, so this is the one value
/// the updater compares against the latest GitHub Release tag.
///
/// # Returns
///
/// The semantic version string, e.g. `"0.1.0"` (no leading `v`).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
