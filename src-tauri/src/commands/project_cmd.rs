/*
 * Commands that enumerate local projects and report aggregate backup scheduling state.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::State;

use crate::error::{BackrCommandError, BackrError};
use crate::project_snapshot_cache::{self, load_snapshot_cache, remote_cache_key, save_snapshot_cache};
use crate::state::AppState;

/// One row in the dashboard project table.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    /// Directory name immediately under the configured projects root.
    pub name: String,
    /// Parsed timestamp from the newest snapshot folder name, if any snapshots exist remotely.
    pub last_backup_at: Option<DateTime<Utc>>,
    /// Count of remote snapshot directories matching the strict naming convention.
    pub snapshot_count: usize,
    /// True when `last_backup_at` / `snapshot_count` came from disk cache (SSH unreachable).
    #[serde(default)]
    pub stats_from_cache: bool,
}

/// Parsed backup cadence information shown in the status chrome.
#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    /// Last persisted successful backup instant (from `[state]` when available).
    pub last_backup_at: Option<DateTime<Utc>>,
    /// Best-effort prediction of the next scheduled trigger instant.
    pub next_backup_at: Option<DateTime<Utc>>,
    /// Whether a backup task is currently mutating remote snapshot storage.
    pub in_progress: bool,
    /// Active project directory name when `in_progress` is true.
    pub active_project: Option<String>,
}

/// Lists immediate child directories of `local.projects_path` as project names.
///
/// # Inputs
///
/// * `probe_remote` — when `Some(true)`, probes SSH for live snapshot listings and refreshes disk
///   cache; when `None` or `Some(false)`, uses **only** local project folders plus
///   [`crate::project_snapshot_cache`] so the dashboard works off-grid without hanging on SSH
///   (after backups or an explicit remote refresh).
///
/// # Returns
///
/// A vector sorted lexicographically.
#[tauri::command]
pub async fn list_projects(
    state: State<'_, Arc<AppState>>,
    probe_remote: Option<bool>,
) -> Result<Vec<ProjectInfo>, BackrCommandError> {
    let probe_remote = probe_remote.unwrap_or(false);
    /* AppState::require_config returns the cloned Config or a NotConfigured error. */
    let cfg = state.require_config().await?;

    let base = Path::new(&cfg.local.projects_path);
    if !base.exists() {
        return Err(BackrCommandError::io(format!(
            "projects path does not exist: {}",
            cfg.local.projects_path
        )));
    }

    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(BackrCommandError::from)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let key = remote_cache_key(&cfg);
    let disk_cache = load_snapshot_cache();
    let mut persisted = if disk_cache.remote_key == key {
        disk_cache.clone()
    } else {
        project_snapshot_cache::SnapshotCacheFile {
            remote_key: key.clone(),
            updated_at: None,
            projects: std::collections::HashMap::new(),
        }
    };
    persisted.remote_key = key.clone();

    let mut out = Vec::new();

    if !probe_remote {
        for name in names {
            match persisted.projects.get(&name) {
                Some(c) => {
                    out.push(ProjectInfo {
                        name,
                        last_backup_at: c.last_backup_at,
                        snapshot_count: c.snapshot_count,
                        stats_from_cache: true,
                    });
                }
                None => {
                    out.push(ProjectInfo {
                        name,
                        last_backup_at: None,
                        snapshot_count: 0,
                        stats_from_cache: false,
                    });
                }
            }
        }
        return Ok(out);
    }

    /* crate::config::known_hosts_path resolves the isolated SSH known_hosts file path. */
    let known_hosts =
        crate::config::known_hosts_path().map_err(|e: BackrError| BackrCommandError::from(e))?;

    for name in names {
        /* ssh::remote_list_snapshot_names lists snapshots via SSH; returns BackrError on failure. */
        match crate::backup::ssh::remote_list_snapshot_names(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &cfg.remote.backup_path,
            &name,
        )
        .await
        {
            Ok(snaps) => {
                let last = snaps.first().and_then(|s| parse_snapshot_timestamp(s));
                persisted.projects.insert(
                    name.clone(),
                    project_snapshot_cache::CachedProjectStats {
                        last_backup_at: last,
                        snapshot_count: snaps.len(),
                    },
                );
                out.push(ProjectInfo {
                    name,
                    last_backup_at: last,
                    snapshot_count: snaps.len(),
                    stats_from_cache: false,
                });
            }
            Err(_) => match persisted.projects.get(&name) {
                Some(c) => {
                    out.push(ProjectInfo {
                        name,
                        last_backup_at: c.last_backup_at,
                        snapshot_count: c.snapshot_count,
                        stats_from_cache: true,
                    });
                }
                None => {
                    out.push(ProjectInfo {
                        name,
                        last_backup_at: None,
                        snapshot_count: 0,
                        stats_from_cache: false,
                    });
                }
            },
        }
    }

    persisted.updated_at = Some(Utc::now());
    if let Err(err) = save_snapshot_cache(&persisted) {
        tracing::warn!("failed to persist project snapshot cache: {err}");
    }

    Ok(out)
}

/// Combines in-memory backup progress with persisted schedule metadata.
///
/// # Returns
///
/// A [`BackupStatus`] snapshot suitable for UI spinners and "next run" copy.
#[tauri::command]
pub async fn get_backup_status(
    state: State<'_, Arc<AppState>>,
) -> Result<BackupStatus, BackrCommandError> {
    /* AppState::require_config returns the cloned Config or a NotConfigured error. */
    let cfg = state.require_config().await?;

    let last_from_state = *state.last_backup_at.lock().await;
    let last_from_file = cfg.state.last_backup_at;
    let last_backup_at = last_from_state.or(last_from_file);

    let next_backup_at = match last_backup_at {
        Some(last) => Some(last + chrono::Duration::hours(cfg.schedule.interval_hours as i64)),
        None => Some(Utc::now() + chrono::Duration::hours(cfg.schedule.interval_hours as i64)),
    };

    Ok(BackupStatus {
        last_backup_at,
        next_backup_at,
        in_progress: state.in_progress.load(std::sync::atomic::Ordering::SeqCst),
        active_project: state.active_project.lock().await.clone(),
    })
}

/// Parses snapshot directory names such as `2026-05-10_09-00-00` into UTC instants.
///
/// # Inputs
///
/// * `name` — snapshot folder basename (validated separately for naming policy).
///
/// # Returns
///
/// `Some` when parsing succeeds.
pub(crate) fn parse_snapshot_timestamp(name: &str) -> Option<DateTime<Utc>> {
    project_snapshot_cache::parse_snapshot_timestamp(name)
}
