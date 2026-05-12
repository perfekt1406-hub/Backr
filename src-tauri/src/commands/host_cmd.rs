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
}

/// Volume summary for [`HostDashboardView`] chrome (`df` best-effort).
#[derive(Debug, Serialize)]
pub struct HostVolumeSummary {
    pub backup_root: String,
    pub bytes_avail: Option<u64>,
    pub bytes_size: Option<u64>,
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
        let last = snaps.first().and_then(|s| parse_snapshot_timestamp(s));
        out.push(HostProjectRow {
            name,
            snapshot_count: snaps.len(),
            last_backup_at: last,
        });
    }

    Ok(out)
}

/// Parses `df -B1` for the filesystem backing `backup_root`.
#[tauri::command]
pub fn host_volume_summary(backup_root: String) -> Result<HostVolumeSummary, String> {
    let path = Path::new(&backup_root);
    if !path.exists() {
        return Err(format!("backup_root does not exist: {}", backup_root));
    }

    let out = Command::new("df")
        .args(["-B1", "--output=avail,size", backup_root.as_str()])
        .output()
        .map_err(|e| e.to_string())?;

    if !out.status.success() {
        return Ok(HostVolumeSummary {
            backup_root,
            bytes_avail: None,
            bytes_size: None,
        });
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    lines.next(); // header
    let line = lines.next().unwrap_or("");
    let cols: Vec<&str> = line.split_whitespace().collect();
    let avail = cols.first().and_then(|s| s.parse::<u64>().ok());
    let size = cols.get(1).and_then(|s| s.parse::<u64>().ok());

    Ok(HostVolumeSummary {
        backup_root,
        bytes_avail: avail,
        bytes_size: size,
    })
}
