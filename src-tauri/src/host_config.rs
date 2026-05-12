/*
 * Purpose: Detect whether this Backr instance should boot into backup-host dashboard mode.
 * Role: Reads `/etc/backr/host.toml` (NAS setup script), optional user config copy, or env overrides.
 */

use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::config::config_path;
use crate::error::BackrError;

/// Host-dashboard bootstrap descriptor (local snapshot tree root).
#[derive(Debug, Clone)]
pub struct HostDashboardMarker {
    pub backup_root: String,
    pub ssh_user: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HostDashboardToml {
    backup_root: String,
    #[serde(default)]
    ssh_user: Option<String>,
}

fn parse_host_toml(raw: &str) -> Option<HostDashboardMarker> {
    let parsed: HostDashboardToml = toml::from_str(raw).ok()?;
    let backup_root = parsed.backup_root.trim().to_string();
    if backup_root.is_empty() {
        return None;
    }
    Some(HostDashboardMarker {
        backup_root,
        ssh_user: parsed.ssh_user,
    })
}

/// Optional marker beside laptop `config.toml` for developers testing host UI.
fn user_host_marker_path() -> Result<PathBuf, BackrError> {
    let mut p = config_path()?;
    p.pop();
    Ok(p.join("host_dashboard.toml"))
}

/// Returns host mode metadata when env, `/etc/backr/host.toml`, or user marker is present.
///
/// # Returns
///
/// `None` when the laptop client flow should run (`config.toml` wizard path).
pub fn read_host_dashboard_marker() -> Option<HostDashboardMarker> {
    if let Ok(root) = std::env::var("BACKR_HOST_BACKUP_ROOT")
        .or_else(|_| std::env::var("BACKR_HOST_DASHBOARD_ROOT"))
    {
        let trimmed = root.trim().to_string();
        if !trimmed.is_empty() && Path::new(&trimmed).is_absolute() {
            let ssh_user = std::env::var("BACKR_HOST_SSH_USER")
                .ok()
                .filter(|s| !s.trim().is_empty());
            return Some(HostDashboardMarker {
                backup_root: trimmed,
                ssh_user,
            });
        }
    }

    if let Ok(raw) = std::fs::read_to_string("/etc/backr/host.toml") {
        if let Some(marker) = parse_host_toml(&raw) {
            let root = Path::new(&marker.backup_root);
            if root.is_dir() {
                return Some(marker);
            }
            tracing::warn!(
                "host.toml backup_root is not a directory: {}",
                marker.backup_root
            );
        }
    }

    let path = user_host_marker_path().ok()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let marker = parse_host_toml(&raw)?;
    let root = Path::new(&marker.backup_root);
    if root.is_dir() {
        return Some(marker);
    }
    tracing::warn!(
        "host_dashboard.toml backup_root is not a directory: {}",
        marker.backup_root
    );

    None
}
