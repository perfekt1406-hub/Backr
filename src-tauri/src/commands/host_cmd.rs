/*
 * Purpose: Read-only snapshot inspection for backup-host machines (local filesystem under backup_root).
 * Role: Powers `HostDashboardView` — listing projects + snapshots and coarse disk usage via `df`.
 */

use std::path::PathBuf;
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::host_config::load_host_marker;

/// Bootstrap routing payload consumed by `App.svelte` before hash routing.
#[derive(Debug, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ShellBootstrapDto {
    Setup,
    Client,
    Host {
        backup_root: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        ssh_user: Option<String>,
    },
}

/// One snapshot directory under a project folder on the backup host.
#[derive(Debug, Serialize)]
pub struct HostSnapshotRowDto {
    pub id: String,
    pub modified_iso: Option<String>,
}

/// One project (top-level directory under `backup_root`).
#[derive(Debug, Serialize)]
pub struct HostProjectRowDto {
    pub name: String,
    pub snapshots: Vec<HostSnapshotRowDto>,
}

/// Best-effort filesystem stats for the volume backing `backup_root` (Linux `df`).
#[derive(Debug, Serialize)]
pub struct HostVolumeSummaryDto {
    pub backup_root: String,
    pub bytes_avail: Option<u64>,
    pub bytes_size: Option<u64>,
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "TRUE" | "YES"))
        .unwrap_or(false)
}

fn resolve_host_paths() -> Result<(PathBuf, Option<String>), String> {
    let marker = load_host_marker()?;
    let raw = std::env::var("BACKR_HOST_BACKUP_ROOT")
        .ok()
        .or_else(|| marker.as_ref().map(|m| m.backup_root.clone()))
        .ok_or_else(|| {
            String::from(
                "host dashboard needs BACKR_HOST_BACKUP_ROOT or /etc/backr/host.toml backup_root",
            )
        })?;

    let root = PathBuf::from(&raw);
    let canon = root
        .canonicalize()
        .map_err(|e| format!("backup_root {raw}: {e}"))?;

    let ssh_user = marker.and_then(|m| m.ssh_user);
    Ok((canon, ssh_user))
}

/// Decides whether the UI should land on setup, normal client, or read-only host dashboard.
///
/// External: `std::env::var` reads optional overrides; [`crate::config::load_config`] checks laptop config.
#[tauri::command]
pub fn resolve_shell_bootstrap() -> Result<ShellBootstrapDto, String> {
    if env_truthy("BACKR_HOST_MODE") {
        let (root, ssh_user) = resolve_host_paths()?;
        return Ok(ShellBootstrapDto::Host {
            backup_root: root.to_string_lossy().into_owned(),
            ssh_user,
        });
    }

    let cfg = crate::config::load_config().map_err(|e| e.to_string())?;
    if cfg.is_some() {
        return Ok(ShellBootstrapDto::Client);
    }

    if let Some(marker) = load_host_marker()? {
        let root = PathBuf::from(&marker.backup_root);
        let canon = root
            .canonicalize()
            .map_err(|e| format!("backup_root {}: {e}", marker.backup_root))?;
        return Ok(ShellBootstrapDto::Host {
            backup_root: canon.to_string_lossy().into_owned(),
            ssh_user: marker.ssh_user.clone(),
        });
    }

    Ok(ShellBootstrapDto::Setup)
}

fn snapshot_mtime_iso(meta: &std::fs::Metadata) -> Option<String> {
    let st = meta.modified().ok()?;
    Some(DateTime::<Utc>::from(st).to_rfc3339())
}

/// Lists immediate project folders and nested snapshot directories under `backup_root`.
///
/// External: `std::fs::read_dir` enumerates directories; paths stay under canonical `backup_root`.
#[tauri::command]
pub fn host_list_snapshot_projects(backup_root: String) -> Result<Vec<HostProjectRowDto>, String> {
    let root = PathBuf::from(&backup_root)
        .canonicalize()
        .map_err(|e| format!("backup_root {backup_root}: {e}"))?;

    let mut projects = Vec::new();

    for ent in std::fs::read_dir(&root).map_err(|e| format!("read_dir {}: {e}", root.display()))? {
        let ent = ent.map_err(|e| format!("read_dir entry: {e}"))?;
        let ft = ent.file_type().map_err(|e| format!("file_type: {e}"))?;
        if !ft.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        let proj_path = root.join(&name);
        let mut snapshots = Vec::new();

        for snap in std::fs::read_dir(&proj_path)
            .map_err(|e| format!("read_dir {}: {e}", proj_path.display()))?
        {
            let snap = snap.map_err(|e| format!("snapshot entry: {e}"))?;
            let sft = snap.file_type().map_err(|e| format!("snapshot type: {e}"))?;
            if !sft.is_dir() {
                continue;
            }
            let sid = snap.file_name().to_string_lossy().into_owned();
            if sid.starts_with('.') {
                continue;
            }
            let meta = snap.metadata().map_err(|e| format!("snapshot meta: {e}"))?;
            snapshots.push(HostSnapshotRowDto {
                id: sid,
                modified_iso: snapshot_mtime_iso(&meta),
            });
        }

        snapshots.sort_by(|a, b| b.id.cmp(&a.id));
        projects.push(HostProjectRowDto { name, snapshots });
    }

    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(projects)
}

/// Parses `df --output=avail,size` for the volume holding `backup_root` (GNU coreutils).
///
/// External: `std::process::Command` runs `/usr/bin/df`; returns `None` fields when parsing fails.
#[tauri::command]
pub fn host_volume_summary(backup_root: String) -> Result<HostVolumeSummaryDto, String> {
    let root = PathBuf::from(&backup_root)
        .canonicalize()
        .map_err(|e| format!("backup_root {backup_root}: {e}"))?;

    let out = Command::new("df")
        .args(["-B1", "--output=avail,size", "--"])
        .arg(&root)
        .output()
        .map_err(|e| format!("df: {e}"))?;

    if !out.status.success() {
        return Ok(HostVolumeSummaryDto {
            backup_root: root.to_string_lossy().into_owned(),
            bytes_avail: None,
            bytes_size: None,
        });
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let _header = lines.next();
    let mut bytes_avail = None;
    let mut bytes_size = None;
    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            bytes_avail = parts[0].parse::<u64>().ok();
            bytes_size = parts[1].parse::<u64>().ok();
            break;
        }
    }

    Ok(HostVolumeSummaryDto {
        backup_root: root.to_string_lossy().into_owned(),
        bytes_avail,
        bytes_size,
    })
}
