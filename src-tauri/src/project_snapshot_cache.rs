/*
 * Purpose: Persist per-project remote snapshot stats locally so the laptop dashboard works offline.
 * Role: Written after successful backups and after optional SSH refreshes; read when the UI avoids probing SSH.
 */

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::BackrError;

/// Disk snapshot of remote listing keyed by Backr remote identity.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnapshotCacheFile {
    #[serde(default)]
    pub remote_key: String,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub projects: HashMap<String, CachedProjectStats>,
}

/// One cached row mirroring [`crate::commands::project_cmd::ProjectInfo`] remote-derived fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedProjectStats {
    pub last_backup_at: Option<DateTime<Utc>>,
    pub snapshot_count: usize,
}

/// Stable key when `[remote]` identity changes — stale cache rows are ignored until refreshed.
pub fn remote_cache_key(cfg: &Config) -> String {
    format!(
        "{}|{}|{}|{}",
        cfg.remote.host, cfg.remote.user, cfg.remote.port, cfg.remote.backup_path
    )
}

/// Absolute path to JSON cache alongside `config.toml`.
///
/// # Returns
///
/// Path under `dirs::config_dir()/backr/snapshot_stats.json`.
pub fn snapshot_cache_path() -> Result<PathBuf, BackrError> {
    let base = dirs::config_dir().ok_or_else(|| {
        BackrError::Config("could not resolve user config directory".into())
    })?;
    Ok(base.join("backr").join("snapshot_stats.json"))
}

/// Loads JSON cache or returns an empty structure when missing or corrupt.
pub fn load_snapshot_cache() -> SnapshotCacheFile {
    let Ok(path) = snapshot_cache_path() else {
        return SnapshotCacheFile::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return SnapshotCacheFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Persists the cache document atomically (temp file + rename).
pub fn save_snapshot_cache(doc: &SnapshotCacheFile) -> Result<(), BackrError> {
    let path = snapshot_cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(BackrError::Io)?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(doc).map_err(|e| BackrError::Msg(e.to_string()))?;
    std::fs::write(&tmp, raw).map_err(BackrError::Io)?;
    std::fs::rename(&tmp, &path).map_err(BackrError::Io)?;
    Ok(())
}

/// Parses snapshot folder names such as `2026-05-10_09-00-00` as UTC instants.
pub fn parse_snapshot_timestamp(name: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(name, "%Y-%m-%d_%H-%M-%S")
        .ok()
        .map(|n| n.and_utc())
}

/// Updates cache after a successful rsync snapshot for one project.
///
/// # Inputs
///
/// * `cfg` — active configuration (remote identity + paths).
/// * `project` — directory name under `local.projects_path`.
/// * `new_snapshot_name` — directory basename written remotely (`YYYY-MM-DD_HH-MM-SS`).
/// * `snapshot_count_after` — total snapshot folders on remote after this run.
pub fn record_backup_success(
    cfg: &Config,
    project: &str,
    new_snapshot_name: &str,
    snapshot_count_after: usize,
) -> Result<(), BackrError> {
    let key = remote_cache_key(cfg);
    let mut disk = load_snapshot_cache();
    if disk.remote_key != key {
        disk.projects.clear();
    }
    disk.remote_key = key;
    let last = parse_snapshot_timestamp(new_snapshot_name);
    disk.projects.insert(
        project.to_string(),
        CachedProjectStats {
            last_backup_at: last,
            snapshot_count: snapshot_count_after,
        },
    );
    disk.updated_at = Some(Utc::now());
    save_snapshot_cache(&disk)?;
    Ok(())
}
