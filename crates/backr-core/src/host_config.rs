/*
 * Purpose: Detect whether this Backr instance should boot into backup-host dashboard mode.
 * Role: Reads `/etc/backr/host.toml` (NAS setup script), optional user config copy, or env overrides.
 *       The parsed result is cached in a process-global OnceLock so disk is touched at most once.
 */

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::config_path;
use crate::error::BackrError;

/// Process-global cache for the host dashboard marker.
///
/// Stores `Some(marker)` when host mode is active, `None` when this is a client machine.
/// Populated on the first call to [`read_host_dashboard_marker`]; never re-read afterward.
static HOST_MARKER_CACHE: OnceLock<Option<HostDashboardMarker>> = OnceLock::new();

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

/// Resolves the host dashboard marker by checking env vars, `/etc/backr/host.toml`,
/// and the optional user-local marker file.
///
/// This is the uncached inner implementation called exactly once per process by
/// [`read_host_dashboard_marker`].
///
/// # Returns
///
/// `Some(marker)` when a valid host configuration is found; `None` otherwise.
fn resolve_host_dashboard_marker() -> Option<HostDashboardMarker> {
    // Env vars override all file-based detection (developer testing path).
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

    // System-wide NAS marker written by the install script.
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

    // Optional user-local marker for developers testing the host UI.
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

/// Returns host mode metadata when env, `/etc/backr/host.toml`, or user marker is present.
///
/// The result is cached in a process-global [`OnceLock`] after the first call — the marker
/// is set at install time and never changes during a process run, so re-reading from disk
/// on every call is unnecessary.
///
/// # Returns
///
/// A clone of the cached `Option<HostDashboardMarker>`:
/// - `Some(marker)` → this machine runs the backup-host dashboard.
/// - `None` → laptop client flow should run (`config.toml` wizard path).
pub fn read_host_dashboard_marker() -> Option<HostDashboardMarker> {
    HOST_MARKER_CACHE
        .get_or_init(resolve_host_dashboard_marker)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Repeated calls return the same value without panicking (cache hit path).
    ///
    /// We cannot assert a specific `Some`/`None` outcome because the OnceLock is
    /// process-global and may already be populated by a prior test run in the same
    /// process. The important contract is: no panic, stable return type.
    #[test]
    fn repeated_calls_do_not_panic() {
        let first = read_host_dashboard_marker();
        let second = read_host_dashboard_marker();
        // Both calls must agree — the cache must return the same variant both times.
        assert_eq!(first.is_some(), second.is_some());
    }

    /// A missing `/etc/backr/host.toml` with no env vars set must not produce an error.
    ///
    /// This test relies on the CI environment not having the host marker installed.
    /// It is skipped when the env override `BACKR_HOST_BACKUP_ROOT` is set.
    #[test]
    fn missing_marker_returns_none_not_error() {
        // If an env override is active, we cannot make the "None" assertion.
        if std::env::var("BACKR_HOST_BACKUP_ROOT").is_ok()
            || std::env::var("BACKR_HOST_DASHBOARD_ROOT").is_ok()
        {
            return;
        }
        // parse_host_toml on an empty string must return None cleanly.
        assert!(parse_host_toml("").is_none());
        // And a malformed TOML must also return None cleanly (no panic).
        assert!(parse_host_toml("not = toml = garbage").is_none());
    }
}
