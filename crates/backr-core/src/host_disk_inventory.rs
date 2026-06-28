/*
 * Purpose: Disk usage inventory for the backup-host dashboard via `du` with JSON TTL cache.
 * Role: Scans top-level project directories under `backup_root`; avoids blocking every refresh when cache is fresh.
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::BackrError;

/// Per-project byte totals from `du -sb` on immediate children of `backup_root`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDiskProjectBytes {
    pub name: String,
    pub bytes: u64,
}

/// Aggregate backup tree sizes plus cache provenance for the host UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDiskInventory {
    pub backup_root: String,
    pub backup_root_bytes: u64,
    pub projects: Vec<HostDiskProjectBytes>,
    /// True when this payload was read from `host_du_cache.json` without a fresh `du` scan.
    pub from_cache: bool,
    pub scanned_at: Option<DateTime<Utc>>,
}

/// Serialized JSON cache matching [`HostDiskInventory`] but keyed by project name for compact merges.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DuCacheFile {
    backup_root: String,
    scanned_at: DateTime<Utc>,
    backup_root_bytes: u64,
    projects: HashMap<String, u64>,
}

/// TTL for accepting cached `du` results without rescanning.
///
/// Reads optional seconds override from `BACKR_HOST_DU_CACHE_SECS` (must parse as positive integer); defaults to 300.
///
/// External: `std::env::var` reads the optional override without allocating unless present.
fn du_cache_ttl() -> Duration {
    std::env::var("BACKR_HOST_DU_CACHE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(300))
}

/// Resolves `~/.config/backr/host_du_cache.json` for persisted `du` totals.
///
/// External: `dirs::config_dir` resolves the platform config directory (e.g. `~/.config` on Linux).
fn du_cache_path() -> Result<PathBuf, BackrError> {
    let base = dirs::config_dir()
        .ok_or_else(|| BackrError::Config("could not resolve user config directory".into()))?;
    Ok(base.join("backr").join("host_du_cache.json"))
}

/// Loads [`DuCacheFile`] from disk when present and JSON-valid.
///
/// External: `serde_json::from_str` deserializes the UTF-8 file body into [`DuCacheFile`].
fn read_cache_file() -> Option<DuCacheFile> {
    let path = du_cache_path().ok()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Atomically writes cache JSON (temp file + rename).
///
/// External: `serde_json::to_string_pretty` serializes [`DuCacheFile`] for human-readable cache files.
fn write_cache_file(doc: &DuCacheFile) -> Result<(), BackrError> {
    let path = du_cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(BackrError::Io)?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(doc).map_err(|e| BackrError::Msg(e.to_string()))?;
    std::fs::write(&tmp, raw).map_err(BackrError::Io)?;
    std::fs::rename(&tmp, &path).map_err(BackrError::Io)?;
    Ok(())
}

/// Runs `du -sb` for one path (GNU coreutils) and parses the leading byte total.
///
/// External: `std::process::Command::output` runs `du` and returns stdout/stderr and exit status.
fn du_sb_one(path: &Path) -> Result<u64, String> {
    let out = Command::new("du")
        .arg("-sb")
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("du failed for {}", path.display()));
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line
        .split_whitespace()
        .next()
        .ok_or_else(|| "empty du output".to_string())?;
    first.parse::<u64>().map_err(|_| "invalid du size".to_string())
}

/// Scans immediate child directories under `backup_root` plus the root itself via `du -sb`.
///
/// External: `std::fs::read_dir` enumerates top-level directories under the backup root.
fn inventory_from_scan(backup_root: &Path) -> Result<DuCacheFile, String> {
    let root_str = backup_root
        .to_str()
        .ok_or_else(|| "backup_root is not valid UTF-8".to_string())?
        .to_string();
    let root_bytes = du_sb_one(backup_root)?;

    let mut names: Vec<String> = std::fs::read_dir(backup_root)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let mut projects = HashMap::new();
    for name in names {
        let p = backup_root.join(&name);
        match du_sb_one(&p) {
            Ok(b) => {
                projects.insert(name, b);
            }
            Err(err) => tracing::warn!("du skipped for {}: {err}", p.display()),
        }
    }

    Ok(DuCacheFile {
        backup_root: root_str,
        scanned_at: Utc::now(),
        backup_root_bytes: root_bytes,
        projects,
    })
}

/// Converts on-disk cache representation into the IPC payload sorted by project name.
fn cache_entry_to_inventory(doc: &DuCacheFile, from_cache: bool) -> HostDiskInventory {
    let mut projects: Vec<HostDiskProjectBytes> = doc
        .projects
        .iter()
        .map(|(name, &bytes)| HostDiskProjectBytes {
            name: name.clone(),
            bytes,
        })
        .collect();
    projects.sort_by(|a, b| a.name.cmp(&b.name));

    HostDiskInventory {
        backup_root: doc.backup_root.clone(),
        backup_root_bytes: doc.backup_root_bytes,
        projects,
        from_cache,
        scanned_at: Some(doc.scanned_at),
    }
}

/// Returns disk inventory from cache when TTL allows, otherwise runs `du`, with stale-cache fallback.
///
/// # Inputs
///
/// * `backup_root` — absolute backup tree root.
/// * `force_refresh` — when true, ignores TTL and rescans (still falls back to stale JSON if `du` fails).
pub fn host_disk_inventory_impl(
    backup_root: String,
    force_refresh: bool,
) -> Result<HostDiskInventory, String> {
    let root_path = Path::new(&backup_root);
    if !root_path.is_dir() {
        return Err(format!("backup_root is not a directory: {}", backup_root));
    }

    let ttl = du_cache_ttl();
    let cached = read_cache_file();
    let now = Utc::now();

    if !force_refresh {
        if let Some(ref doc) = cached {
            if doc.backup_root == backup_root {
                let age = now.signed_duration_since(doc.scanned_at);
                if age.to_std().unwrap_or(Duration::MAX) <= ttl {
                    return Ok(cache_entry_to_inventory(doc, true));
                }
            }
        }
    }

    match inventory_from_scan(root_path) {
        Ok(doc) => {
            if let Err(err) = write_cache_file(&doc) {
                tracing::warn!("failed to write host_du_cache.json: {err}");
            }
            Ok(cache_entry_to_inventory(&doc, false))
        }
        Err(scan_err) => {
            tracing::warn!("host disk scan failed: {scan_err}");
            if let Some(doc) = cached.filter(|d| d.backup_root == backup_root) {
                Ok(cache_entry_to_inventory(&doc, true))
            } else {
                Err(scan_err)
            }
        }
    }
}
