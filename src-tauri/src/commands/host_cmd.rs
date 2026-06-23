/*
 * Tauri commands for backup-host dashboard bootstrap plus local filesystem introspection.
 */

use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::host_config::read_host_dashboard_marker;
use crate::project_snapshot_cache::parse_snapshot_timestamp;

/// JSON payload consumed by `resolve_shell_bootstrap` — determines initial router destination.
#[derive(Debug, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ShellBootstrap {
    Setup,
    Client,
    Host {
        backup_root: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        ssh_user: Option<String>,
    },
}

/// One project folder visible under `backup_root` with coarse snapshot stats from disk.
#[derive(Debug, Serialize)]
pub struct HostProjectRow {
    pub name: String,
    pub snapshot_count: usize,
    pub last_backup_at: Option<DateTime<Utc>>,
    /// Newest snapshot directory names (up to three), sorted newest-first.
    pub recent_snapshots: Vec<String>,
}

/// Volume summary for [`HostDashboardView`] chrome (`df` best-effort; describes the whole filesystem).
#[derive(Debug, Serialize)]
pub struct HostVolumeSummary {
    pub backup_root: String,
    pub bytes_avail: Option<u64>,
    pub bytes_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<String>,
}

/// Chooses laptop setup vs client vs NAS-local dashboard before SPA routing completes.
#[tauri::command]
pub fn resolve_shell_bootstrap() -> Result<ShellBootstrap, String> {
    if let Some(marker) = read_host_dashboard_marker() {
        let root = Path::new(&marker.backup_root);
        if root.is_dir() {
            return Ok(ShellBootstrap::Host {
                backup_root: marker.backup_root,
                ssh_user: marker.ssh_user,
            });
        }
        tracing::warn!(
            "host_dashboard marker present but backup_root is not a directory: {}",
            marker.backup_root
        );
    }

    match crate::config::load_config() {
        Ok(Some(_)) => Ok(ShellBootstrap::Client),
        Ok(None) => Ok(ShellBootstrap::Setup),
        Err(err) => {
            tracing::warn!("resolve_shell_bootstrap: load_config failed: {err}");
            Ok(ShellBootstrap::Setup)
        }
    }
}

/// Lists snapshot subdirectory names for `project_path`, newest valid snapshot names first.
///
/// # Returns
///
/// Sorted folder names with parseable snapshot timestamps, newest lexicographic/timestamp order first.
fn snapshot_dirs(project_path: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(project_path) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| parse_snapshot_timestamp(n).is_some())
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    names
}

/// Lists projects by scanning `backup_root/<project>/<snapshot>/` locally on the NAS machine.
#[tauri::command]
pub fn host_list_snapshot_projects(backup_root: String) -> Result<Vec<HostProjectRow>, String> {
    let base = Path::new(&backup_root);
    if !base.is_dir() {
        return Err(format!("backup_root is not a directory: {}", backup_root));
    }

    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let mut out = Vec::new();
    for name in names {
        let project_path = base.join(&name);
        let snaps = snapshot_dirs(&project_path);
        let snapshot_count = snaps.len();
        let last_backup_at = snaps.first().and_then(|s| parse_snapshot_timestamp(s));
        let recent_snapshots = snaps.into_iter().take(3).collect();
        out.push(HostProjectRow {
            name,
            snapshot_count,
            last_backup_at,
            recent_snapshots,
        });
    }

    Ok(out)
}

/// Parses GNU `df --output=source,target,avail,size,used,pcent` data row when six columns are present.
///
/// # Inputs
///
/// * `line` — non-header whitespace-separated row from `df -B1`.
///
/// # Returns
///
/// Tuple of device, mount point, avail/size/used bytes, and percent-used token if parsing succeeds.
fn parse_df_full_row(line: &str) -> Option<(String, String, u64, u64, u64, String)> {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 6 {
        return None;
    }
    let filesystem_source = cols.first()?.to_string();
    let mount_point = cols.get(1)?.to_string();
    let bytes_avail = cols.get(2)?.parse().ok()?;
    let bytes_size = cols.get(3)?.parse().ok()?;
    let used_bytes = cols.get(4)?.parse().ok()?;
    let used_percent = cols.get(5)?.to_string();
    Some((
        filesystem_source,
        mount_point,
        bytes_avail,
        bytes_size,
        used_bytes,
        used_percent,
    ))
}

/// Parses legacy two-column `df --output=avail,size` row.
///
/// # Returns
///
/// `(bytes_avail, bytes_size)` with missing values as `None`.
fn parse_df_legacy_row(line: &str) -> (Option<u64>, Option<u64>) {
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() < 2 {
        return (None, None);
    }
    let avail = cols.first().and_then(|s| s.parse().ok());
    let size = cols.get(1).and_then(|s| s.parse().ok());
    (avail, size)
}

/// Builds [`HostVolumeSummary`] by probing `df` against `backup_root`, preferring GNU `--output` enrichments.
///
/// # Inputs
///
/// * `backup_root` — path whose containing filesystem should be queried (may be file or directory).
#[tauri::command]
pub fn host_volume_summary(backup_root: String) -> Result<HostVolumeSummary, String> {
    let path = Path::new(&backup_root);
    if !path.exists() {
        return Err(format!("backup_root does not exist: {}", backup_root));
    }

    let enriched = Command::new("df")
        .args([
            "-B1",
            "--output=source,target,avail,size,used,pcent",
            backup_root.as_str(),
        ])
        .output()
        .map_err(|e| e.to_string())?;

    if enriched.status.success() {
        let text = String::from_utf8_lossy(&enriched.stdout);
        let mut lines = text.lines();
        lines.next(); // header
        let line = lines.next().unwrap_or("");
        if let Some((filesystem_source, mount_point, bytes_avail, bytes_size, used_bytes, used_percent)) =
            parse_df_full_row(line)
        {
            return Ok(HostVolumeSummary {
                backup_root,
                bytes_avail: Some(bytes_avail),
                bytes_size: Some(bytes_size),
                filesystem_source: Some(filesystem_source),
                mount_point: Some(mount_point),
                used_bytes: Some(used_bytes),
                used_percent: Some(used_percent),
            });
        }
    }

    let legacy = Command::new("df")
        .args(["-B1", "--output=avail,size", backup_root.as_str()])
        .output()
        .map_err(|e| e.to_string())?;

    if !legacy.status.success() {
        return Ok(HostVolumeSummary {
            backup_root,
            bytes_avail: None,
            bytes_size: None,
            filesystem_source: None,
            mount_point: None,
            used_bytes: None,
            used_percent: None,
        });
    }

    let text = String::from_utf8_lossy(&legacy.stdout);
    let mut lines = text.lines();
    lines.next();
    let line = lines.next().unwrap_or("");
    let (bytes_avail, bytes_size) = parse_df_legacy_row(line);

    Ok(HostVolumeSummary {
        backup_root,
        bytes_avail,
        bytes_size,
        filesystem_source: None,
        mount_point: None,
        used_bytes: None,
        used_percent: None,
    })
}

/// Runs `du`-backed disk inventory off the async runtime to avoid blocking the UI thread.
///
/// # Inputs
///
/// * `backup_root` — backup tree root to scan.
/// * `force_refresh` — bypass TTL and attempt a fresh `du` pass when true.
///
/// External: `tauri::async_runtime::spawn_blocking` schedules [`crate::host_disk_inventory::host_disk_inventory_impl`].
#[tauri::command]
pub async fn host_disk_inventory(
    backup_root: String,
    force_refresh: bool,
) -> Result<crate::host_disk_inventory::HostDiskInventory, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::host_disk_inventory::host_disk_inventory_impl(backup_root, force_refresh)
    })
    .await
    .map_err(|e| format!("disk inventory task failed: {e}"))?
}

/// Reports authorized_keys path + pubkey count for the backup SSH account (host Trust page).
///
/// External: delegates to [`crate::host_trust::host_trust_status_impl`] (inputs: none; outputs: [`crate::host_trust::HostTrustStatus`]).
#[tauri::command]
pub fn host_trust_status() -> Result<crate::host_trust::HostTrustStatus, String> {
    crate::host_trust::host_trust_status_impl()
}

/// Appends one validated pubkey line to authorized_keys, or returns sudo fallback commands for the operator.
///
/// External: delegates to [`crate::host_trust::host_append_authorized_pubkey_impl`] (inputs: pubkey text; outputs: [`crate::host_trust::HostTrustAppendResult`]).
#[tauri::command]
pub fn host_append_authorized_pubkey(
    pubkey_line: String,
) -> Result<crate::host_trust::HostTrustAppendResult, String> {
    crate::host_trust::host_append_authorized_pubkey_impl(pubkey_line)
}

/// Lists every parsed pubkey entry in authorized_keys for the host Settings trusted-keys list.
///
/// External: delegates to [`crate::host_trust::host_list_authorized_pubkeys_impl`] (inputs: none; outputs: Vec<[`crate::host_trust::AuthorizedPubkeyEntry`]>).
#[tauri::command]
pub fn host_list_authorized_pubkeys() -> Result<Vec<crate::host_trust::AuthorizedPubkeyEntry>, String> {
    crate::host_trust::host_list_authorized_pubkeys_impl()
}

/// Removes one pubkey line (identified by exact raw_line match) from authorized_keys.
///
/// External: delegates to [`crate::host_trust::host_remove_authorized_pubkey_impl`] (inputs: raw_line; outputs: [`crate::host_trust::HostRemovePubkeyResult`]).
#[tauri::command]
pub fn host_remove_authorized_pubkey(
    raw_line: String,
) -> Result<crate::host_trust::HostRemovePubkeyResult, String> {
    crate::host_trust::host_remove_authorized_pubkey_impl(raw_line)
}
