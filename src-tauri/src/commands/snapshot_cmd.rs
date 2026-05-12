/*
 * Snapshot browsing and restore commands backed by remote `find` helpers.
 */

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::backup::rsync;
use crate::backup::ssh::{self, is_valid_snapshot_name};
use crate::config::{self};
use crate::error::BackrError;
use crate::progress_sink::{AppEmitProgress, SharedProgress};
use crate::state::AppState;

/// One snapshot row for project timelines.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotEntry {
    /// Remote directory name (timestamp string).
    pub name: String,
    /// Best-effort directory mtime in seconds since UNIX epoch (if discoverable).
    pub modified_unix: Option<f64>,
    /// Total size in bytes (not computed here; reserved for future `du` integration).
    pub size_bytes: Option<u64>,
}

/// One row in the lazy file tree (`list_files` payload).
#[derive(Debug, Clone, Serialize)]
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

/// UTF-8 text preview payload for snapshot file reads (bounded by [`SNAPSHOT_READ_MAX_BYTES`]).
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotFileContents {
    /// Decoded body suitable for monospace rendering.
    pub text: String,
    /// True when the remote stream hit the byte cap — tail omitted.
    pub truncated: bool,
}

/// One project's bulk-restore outcome for [`restore_all_projects`].
#[derive(Debug, Clone, Serialize)]
pub struct RestoreEveryProjectRow {
    /// Local project directory name under `local.projects_path`.
    pub project: String,
    /// Absolute restore folders written for that project (newest snapshot first).
    pub destinations: Vec<String>,
}

/// Largest snapshot file chunk forwarded to the UI (512 KiB text preview cap).
const SNAPSHOT_READ_MAX_BYTES: u64 = 512 * 1024;

/// Home-relative directory basename (`Projects-…`) before collision uniquification.
///
/// Standard snapshot IDs (`YYYY-MM-DD_HH-MM-SS`) embed time already → `Projects-<id>`.
/// Non-standard names append the current UTC stamp so restores stay traceable and unique.
fn restore_destination_folder_base(snapshot: &str) -> String {
    if is_valid_snapshot_name(snapshot) {
        format!("Projects-{snapshot}")
    } else {
        let stamp = Utc::now().format("%Y-%m-%d_%H-%M-%S");
        let mut safe: String = snapshot
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(120)
            .collect();
        while safe.ends_with('_') {
            safe.pop();
        }
        let suffix = if safe.is_empty() {
            "snapshot".to_string()
        } else {
            safe
        };
        format!("Projects-{suffix}-{stamp}")
    }
}

/// Runs one rsync restore for `snapshot`; shared by single-restore and bulk-restore commands.
async fn restore_single_snapshot_to_home(
    app: &AppHandle,
    cfg: &config::Config,
    known_hosts: &Path,
    project: &str,
    snapshot: &str,
) -> Result<String, BackrError> {
    let remote_dir =
        rsync::absolute_remote_snapshot_path(&cfg.remote.backup_path, project, snapshot);
    let remote_url = format!(
        "{}@{}:{}/",
        cfg.remote.user, cfg.remote.host, remote_dir
    );

    let home =
        dirs::home_dir().ok_or_else(|| BackrError::Msg("could not resolve home directory".into()))?;
    let base_local = home.join(restore_destination_folder_base(snapshot));
    let destination = uniquify_path(&base_local)?;

    std::fs::create_dir_all(&destination).map_err(BackrError::Io)?;

    let sink: SharedProgress = Arc::new(AppEmitProgress::new(app.clone()));
    rsync::rsync_restore_snapshot(
        sink,
        &cfg.remote.ssh_key,
        known_hosts,
        &remote_url,
        cfg.remote.port,
        &destination,
    )
    .await?;

    Ok(destination.to_string_lossy().into_owned())
}

/// Lexicographic folder names immediately under the configured projects root.
///
/// # Inputs
///
/// * `base` — `local.projects_path`; must exist as a directory.
///
/// # Returns
///
/// Sorted directory basenames only (no files).
fn local_child_directory_names(base: &Path) -> Result<Vec<String>, BackrError> {
    if !base.exists() {
        return Err(BackrError::Msg(format!(
            "projects path does not exist: {}",
            base.display()
        )));
    }
    let mut names: Vec<String> = std::fs::read_dir(base)
        .map_err(BackrError::Io)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// Restores every strictly-named remote snapshot for `project`, newest first.
///
/// External: `ssh::remote_list_snapshot_names` then [`restore_single_snapshot_to_home`] per ID.
async fn restore_every_valid_snapshot_for_project(
    app: &AppHandle,
    cfg: &config::Config,
    known_hosts: &Path,
    project: &str,
) -> Result<Vec<String>, BackrError> {
    let names = ssh::remote_list_snapshot_names(
        &cfg.remote.ssh_key,
        known_hosts,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        project,
    )
    .await?;

    let mut snapshots: Vec<String> = names
        .into_iter()
        .filter(|n| is_valid_snapshot_name(n))
        .collect();
    snapshots.sort_by(|a, b| b.cmp(a));

    let mut paths = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        paths.push(
            restore_single_snapshot_to_home(app, cfg, known_hosts, project, &snapshot).await?,
        );
    }
    Ok(paths)
}

/// Lists snapshot folders for a project on the remote host, newest first.
#[tauri::command]
pub async fn list_snapshots(
    state: State<'_, Arc<AppState>>,
    project: String,
) -> Result<Vec<SnapshotEntry>, String> {
    let cfg = state
        .config
        .lock()
        .await
        .clone()
        .ok_or_else(|| "not configured".to_string())?;
    let known = config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;
    let names = ssh::remote_list_snapshot_names(
        &cfg.remote.ssh_key,
        &known,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        &project,
    )
    .await
    .map_err(|e: BackrError| e.to_string())?;

    let mut out = Vec::new();
    for name in names {
        if !is_valid_snapshot_name(&name) {
            continue;
        }
        out.push(SnapshotEntry {
            name,
            modified_unix: None,
            size_bytes: None,
        });
    }
    Ok(out)
}

/// Lists immediate children for a path inside a snapshot using `find -maxdepth 1`.
#[tauri::command]
pub async fn list_files(
    state: State<'_, Arc<AppState>>,
    project: String,
    snapshot: String,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let cfg = state
        .config
        .lock()
        .await
        .clone()
        .ok_or_else(|| "not configured".to_string())?;
    let known = config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;
    let normalized = if path.is_empty() || path == "." {
        String::new()
    } else {
        path.trim().trim_start_matches('/').to_string()
    };

    let rows = ssh::remote_list_children(
        &cfg.remote.ssh_key,
        &known,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        &project,
        &snapshot,
        &normalized,
    )
    .await
    .map_err(|e: BackrError| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        let is_dir = row.file_type == 'd';
        out.push(FileEntry {
            name: row.name,
            is_dir,
            size: row.size,
            modified_unix: Some(row.mtime_unix),
        });
    }
    Ok(out)
}

/// Reads a UTF-8 text slice from a snapshot file via remote `head -c`.
///
/// # Inputs
///
/// * `relative_path` — file location relative to snapshot root (`README.md`, `src/main.rs`).
///
/// # Returns
///
/// [`SnapshotFileContents`] with optional truncation notice when `head` hits the byte ceiling.
#[tauri::command]
pub async fn read_snapshot_file(
    state: State<'_, Arc<AppState>>,
    project: String,
    snapshot: String,
    relative_path: String,
) -> Result<SnapshotFileContents, String> {
    if !is_valid_snapshot_name(&snapshot) {
        return Err("invalid snapshot folder name".into());
    }
    let cfg = state
        .config
        .lock()
        .await
        .clone()
        .ok_or_else(|| "not configured".to_string())?;
    let known = config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;
    let normalized = relative_path.trim().trim_start_matches('/').to_string();
    if normalized.contains("..") {
        return Err("invalid path".into());
    }

    let bytes = ssh::remote_read_file_bytes(
        &cfg.remote.ssh_key,
        &known,
        &cfg.remote.host,
        &cfg.remote.user,
        cfg.remote.port,
        &cfg.remote.backup_path,
        &project,
        &snapshot,
        &normalized,
        SNAPSHOT_READ_MAX_BYTES,
    )
    .await
    .map_err(|e: BackrError| e.to_string())?;

    let truncated = bytes.len() as u64 >= SNAPSHOT_READ_MAX_BYTES;
    let text = String::from_utf8(bytes).map_err(|_| {
        "file is not valid UTF-8 (binary files cannot be previewed)".to_string()
    })?;

    Ok(SnapshotFileContents { text, truncated })
}

/// Restores an entire snapshot under home (`~/Projects-<snapshot>` or stamped basename if needed),
/// then collision-suffixes (`-1`, …) when that folder already exists.
///
/// # Returns
///
/// The absolute local directory path written by rsync.
#[tauri::command]
pub async fn restore_snapshot(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    project: String,
    snapshot: String,
) -> Result<String, String> {
    let cfg = state
        .config
        .lock()
        .await
        .clone()
        .ok_or_else(|| "not configured".to_string())?;
    let known = config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;

    restore_single_snapshot_to_home(&app, &cfg, &known, &project, &snapshot)
        .await
        .map_err(|e: BackrError| e.to_string())
}

/// Restores every indexed snapshot for `project` sequentially (newest remote listing order).
///
/// # Returns
///
/// Absolute paths written for each snapshot, in the order restores ran.
#[tauri::command]
pub async fn restore_all_snapshots(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    project: String,
) -> Result<Vec<String>, String> {
    let cfg = state
        .config
        .lock()
        .await
        .clone()
        .ok_or_else(|| "not configured".to_string())?;
    let known = config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;

    restore_every_valid_snapshot_for_project(&app, &cfg, &known, &project)
        .await
        .map_err(|e: BackrError| e.to_string())
}

/// Restores all valid snapshots for every immediate child project directory (lexicographic order).
///
/// Projects with no matching remote snapshots are omitted from the result vector.
///
/// # Returns
///
/// Per-project destination paths in the same order restores ran within each project (newest snapshot first).
#[tauri::command]
pub async fn restore_all_projects(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<RestoreEveryProjectRow>, String> {
    let cfg = state
        .config
        .lock()
        .await
        .clone()
        .ok_or_else(|| "not configured".to_string())?;
    let known = config::known_hosts_path().map_err(|e: BackrError| e.to_string())?;
    let base = Path::new(&cfg.local.projects_path);
    let projects = local_child_directory_names(base).map_err(|e: BackrError| e.to_string())?;

    let mut rows = Vec::new();
    for project in projects {
        let destinations =
            restore_every_valid_snapshot_for_project(&app, &cfg, &known, &project)
                .await
                .map_err(|e: BackrError| e.to_string())?;
        if !destinations.is_empty() {
            rows.push(RestoreEveryProjectRow {
                project,
                destinations,
            });
        }
    }

    Ok(rows)
}

/// Appends `-1`, `-2`, ... when `path` already exists on disk.
///
/// # Inputs
///
/// * `path` — desired destination directory.
///
/// # Returns
///
/// A non-existing directory path adjacent to `path`.
fn uniquify_path(path: &Path) -> Result<PathBuf, BackrError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let parent = path
        .parent()
        .ok_or_else(|| BackrError::Msg("invalid restore parent".into()))?;
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| BackrError::Msg("invalid restore path".into()))?;
    for i in 1..10_000u32 {
        let candidate = parent.join(format!("{file_name}-{i}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(BackrError::Msg(
        "could not find a free restore folder suffix".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn uniq_tmp(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// When destination exists, restores pick the next free `-n` sibling directory.
    #[test]
    fn uniquify_existing_gets_incremental_suffix() {
        let root = uniq_tmp("backr-uniq-root");
        fs::create_dir_all(&root).unwrap();
        let base = root.join("Projects-2026-05-11_09-30-45");
        fs::create_dir(&base).unwrap();
        let got = uniquify_path(&base).unwrap();
        assert_eq!(got, root.join("Projects-2026-05-11_09-30-45-1"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn uniquify_nonexistent_keeps_requested_path() {
        let root = uniq_tmp("backr-uniq-free");
        fs::create_dir_all(&root).unwrap();
        let base = root.join("Projects-unused");
        let got = uniquify_path(&base).unwrap();
        assert_eq!(got, base);
        fs::remove_dir_all(&root).unwrap();
    }
}
