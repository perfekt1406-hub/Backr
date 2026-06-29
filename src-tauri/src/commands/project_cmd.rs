/*
 * Commands that enumerate local projects and report aggregate backup scheduling state.
 *
 * Thin IPC proxies delegating to the backrd daemon.  The function signatures are
 * kept identical to preserve the frontend `invoke()` call contract.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::BackrCommandError;
use crate::state::AppState;

/// One row in the dashboard project table.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Lists projects and their snapshot stats from the daemon.
///
/// # Inputs
///
/// * `state`        — managed [`AppState`] (unused by proxy; kept for signature compatibility).
/// * `probe_remote` — when `Some(true)`, asks the daemon to probe SSH for live snapshot counts.
///
/// # Returns
///
/// A vector sorted lexicographically.
#[tauri::command]
pub async fn list_projects(
    _state: State<'_, Arc<AppState>>,
    probe_remote: Option<bool>,
) -> Result<Vec<ProjectInfo>, BackrCommandError> {
    let v = crate::ipc_client::send(
        "list_projects",
        serde_json::json!({ "probe_remote": probe_remote }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize project list: {e}"))
    })
}

/// Returns current backup progress and schedule status from the daemon.
///
/// # Inputs
///
/// * `state` — managed [`AppState`] (unused by proxy; kept for signature compatibility).
///
/// # Returns
///
/// A [`BackupStatus`] snapshot suitable for UI spinners and "next run" copy.
#[tauri::command]
pub async fn get_backup_status(
    _state: State<'_, Arc<AppState>>,
) -> Result<BackupStatus, BackrCommandError> {
    let v =
        crate::ipc_client::send("get_backup_status", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize backup status: {e}"))
    })
}
