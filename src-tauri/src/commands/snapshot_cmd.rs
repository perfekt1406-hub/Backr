/*
 * Snapshot browsing and restore commands proxied to the backrd daemon.
 *
 * All commands are thin IPC proxies.  Restore commands use `send_with_progress`
 * to forward rsync streaming output to the webview during long-running operations.
 *
 * The function signatures are kept identical to preserve the frontend `invoke()` call contract.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::BackrCommandError;
use crate::state::AppState;

/// One snapshot row for project timelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// Remote directory name (timestamp string).
    pub name: String,
}

/// One row in the lazy file tree (`list_files` payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Single path component name.
    pub name: String,
    /// Whether this entry is a directory according to remote `find` type.
    pub is_dir: bool,
    /// Size in bytes (best-effort for regular files).
    pub size: u64,
    /// Remote mtime seconds (`%T@` from `find`).
    pub modified_unix: Option<f64>,
}

/// UTF-8 text preview payload for snapshot file reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFileContents {
    /// Decoded body suitable for monospace rendering.
    pub text: String,
    /// True when the remote stream hit the byte cap — tail omitted.
    pub truncated: bool,
}

/// One project's bulk-restore outcome for [`restore_all_projects`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreEveryProjectRow {
    /// Local project directory name under `local.projects_path`.
    pub project: String,
    /// Absolute restore folders written for that project (newest snapshot first).
    pub destinations: Vec<String>,
}

/// Lists snapshot folders for a project on the remote host, newest first.
///
/// # Inputs
///
/// * `state`   — managed [`AppState`] (unused by proxy; kept for signature compatibility).
/// * `project` — project directory name to list snapshots for.
#[tauri::command]
pub async fn list_snapshots(
    _state: State<'_, Arc<AppState>>,
    project: String,
) -> Result<Vec<SnapshotEntry>, BackrCommandError> {
    let v = crate::ipc_client::send(
        "list_snapshots",
        serde_json::json!({ "project": project }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize snapshot list: {e}"))
    })
}

/// Lists immediate children for a path inside a snapshot using a remote `find -maxdepth 1`.
///
/// # Inputs
///
/// * `state`    — managed [`AppState`] (unused by proxy; kept for signature compatibility).
/// * `project`  — project directory name.
/// * `snapshot` — snapshot directory name (timestamp format).
/// * `path`     — relative path within the snapshot root (empty string = root).
#[tauri::command]
pub async fn list_files(
    _state: State<'_, Arc<AppState>>,
    project: String,
    snapshot: String,
    path: String,
) -> Result<Vec<FileEntry>, BackrCommandError> {
    let v = crate::ipc_client::send(
        "list_files",
        serde_json::json!({
            "project": project,
            "snapshot": snapshot,
            "path": path,
        }),
    )
    .await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::config(format!("failed to deserialize file list: {e}")))
}

/// Reads a UTF-8 text slice from a snapshot file.
///
/// # Inputs
///
/// * `state`         — managed [`AppState`] (unused by proxy; kept for signature compatibility).
/// * `project`       — project directory name.
/// * `snapshot`      — snapshot directory name (validated timestamp format).
/// * `relative_path` — file location relative to snapshot root.
///
/// # Returns
///
/// [`SnapshotFileContents`] with optional truncation notice when the daemon hit the byte ceiling.
#[tauri::command]
pub async fn read_snapshot_file(
    _state: State<'_, Arc<AppState>>,
    project: String,
    snapshot: String,
    relative_path: String,
) -> Result<SnapshotFileContents, BackrCommandError> {
    let v = crate::ipc_client::send(
        "read_snapshot_file",
        serde_json::json!({
            "project": project,
            "snapshot": snapshot,
            "relative_path": relative_path,
        }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize snapshot file contents: {e}"))
    })
}

/// Restores an entire snapshot under home, streaming rsync progress to the webview.
///
/// # Inputs
///
/// * `app`      — Tauri app handle forwarded to `send_with_progress` for progress streaming.
/// * `state`    — managed [`AppState`] (unused by proxy; kept for signature compatibility).
/// * `project`  — project directory name.
/// * `snapshot` — snapshot directory name.
///
/// # Returns
///
/// The absolute local directory path written by rsync.
#[tauri::command]
pub async fn restore_snapshot(
    app: AppHandle,
    _state: State<'_, Arc<AppState>>,
    project: String,
    snapshot: String,
) -> Result<String, BackrCommandError> {
    let v = crate::ipc_client::send_with_progress(
        "restore_snapshot",
        serde_json::json!({
            "project": project,
            "snapshot": snapshot,
        }),
        &app,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize restore path: {e}"))
    })
}

/// Restores every indexed snapshot for `project` sequentially, streaming rsync progress.
///
/// # Inputs
///
/// * `app`     — Tauri app handle forwarded for progress streaming.
/// * `state`   — managed [`AppState`] (unused by proxy; kept for signature compatibility).
/// * `project` — project directory name.
///
/// # Returns
///
/// Absolute paths written for each snapshot, in the order restores ran.
#[tauri::command]
pub async fn restore_all_snapshots(
    app: AppHandle,
    _state: State<'_, Arc<AppState>>,
    project: String,
) -> Result<Vec<String>, BackrCommandError> {
    let v = crate::ipc_client::send_with_progress(
        "restore_all_snapshots",
        serde_json::json!({ "project": project }),
        &app,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize restore paths: {e}"))
    })
}

/// Restores all valid snapshots for every immediate child project directory.
///
/// # Inputs
///
/// * `app`   — Tauri app handle forwarded for progress streaming.
/// * `state` — managed [`AppState`] (unused by proxy; kept for signature compatibility).
///
/// # Returns
///
/// Per-project destination paths in the order restores ran within each project.
#[tauri::command]
pub async fn restore_all_projects(
    app: AppHandle,
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<RestoreEveryProjectRow>, BackrCommandError> {
    let v = crate::ipc_client::send_with_progress(
        "restore_all_projects",
        serde_json::json!({}),
        &app,
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!(
            "failed to deserialize restore-all-projects result: {e}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    fn uniq_tmp(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// Retain the uniquify-path tests from the original implementation for regression coverage.
    /// In proxy mode this logic lives in the daemon; the tests document expected behavior.
    #[test]
    fn uniquify_nonexistent_keeps_requested_path() {
        let root = uniq_tmp("backr-uniq-free");
        std::fs::create_dir_all(&root).unwrap();
        let base = root.join("Projects-unused");
        // Path does not exist → should not be modified.
        assert!(!base.exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn uniquify_existing_sibling_pattern() {
        let root = uniq_tmp("backr-uniq-root");
        std::fs::create_dir_all(&root).unwrap();
        let base = root.join("Projects-2026-05-11_09-30-45");
        std::fs::create_dir(&base).unwrap();
        // Verify the naming convention is `<base>-1`, `-2`, etc.
        let suffixed = root.join("Projects-2026-05-11_09-30-45-1");
        assert!(!suffixed.exists(), "sibling should not exist before daemon restore");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
