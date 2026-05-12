/*
 * Commands that enumerate local projects and report aggregate backup scheduling state.
 */

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use tauri::State;

use crate::error::BackrError;
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
/// # Returns
///
/// A vector sorted lexicographically; remote snapshot metadata is hydrated via SSH when configured.
#[tauri::command]
pub async fn list_projects(state: State<'_, Arc<AppState>>) -> Result<Vec<ProjectInfo>, String> {
    let cfg = {
        let g = state.config.lock().await;
        g.clone()
            .ok_or_else(|| "configure the application before listing projects".to_string())?
    };
    let base = Path::new(&cfg.local.projects_path);
    if !base.exists() {
        return Err(format!(
            "projects path does not exist: {}",
            cfg.local.projects_path
        ));
    }
    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(|e: std::io::Error| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();

    let known_hosts = crate::config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;
    let mut out = Vec::new();
    for name in names {
        let snaps = crate::backup::ssh::remote_list_snapshot_names(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &cfg.remote.backup_path,
            &name,
        )
        .await
        .unwrap_or_default();

        let last = snaps.first().and_then(|s| parse_snapshot_timestamp(s));
        out.push(ProjectInfo {
            name,
            last_backup_at: last,
            snapshot_count: snaps.len(),
        });
    }
    Ok(out)
}

/// Combines in-memory backup progress with persisted schedule metadata.
///
/// # Returns
///
/// A [`BackupStatus`] snapshot suitable for UI spinners and “next run” copy.
#[tauri::command]
pub async fn get_backup_status(state: State<'_, Arc<AppState>>) -> Result<BackupStatus, String> {
    let cfg_opt = state.config.lock().await.clone();
    let cfg = cfg_opt.ok_or_else(|| "not configured".to_string())?;
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
    let naive = chrono::NaiveDateTime::parse_from_str(name, "%Y-%m-%d_%H-%M-%S").ok()?;
    Some(Utc.from_utc_datetime(&naive))
}
