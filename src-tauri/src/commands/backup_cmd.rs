/*
 * Backup command surface: on-demand runs, scheduler hook points, and rsync orchestration.
 * Uses an `AtomicBool` compare-exchange guard to prevent overlapping backup jobs.
 */

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::backup::rsync::{self, remote_snapshot_dest_folder};
use crate::backup::ssh;
use crate::backup::validate::validate_remote_component;
use crate::config::{self, Config};
use crate::error::BackrError;
use crate::progress_sink::{AppEmitProgress, SharedProgress};
use crate::state::AppState;

/// Clears the in-progress flag when a worker finishes.
struct InProgressDrop {
    state: Arc<AppState>,
}

impl Drop for InProgressDrop {
    fn drop(&mut self) {
        self.state.in_progress.store(false, Ordering::SeqCst);
    }
}

/// Visible to VM/integration tests via a custom [`SharedProgress`] sink; skips tray refreshes.
pub async fn execute_backup_cycle_with_sink(
    sink: SharedProgress,
    state: &Arc<AppState>,
    project: Option<String>,
) -> Result<(), BackrError> {
    let cfg = {
        let g = state.config.lock().await;
        g.clone()
            .ok_or_else(|| BackrError::Config("application is not configured".into()))?
    };

    let known_hosts = config::known_hosts_path()?;
    let projects = resolve_targets(&cfg, project.as_deref())?;
    if projects.is_empty() {
        sink.backup_progress_line("[backr] no local projects found for backup".into());
        return Ok(());
    }

    for project_name in projects {
        {
            let mut ap = state.active_project.lock().await;
            *ap = Some(project_name.clone());
        }

        let local_dir = Path::new(&cfg.local.projects_path).join(&project_name);
        if !local_dir.exists() {
            return Err(BackrError::Msg(format!(
                "local project path missing: {}",
                local_dir.display()
            )));
        }

        let snapshots = ssh::remote_list_snapshot_names(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &cfg.remote.backup_path,
            &project_name,
        )
        .await
        .unwrap_or_default();

        let project_remote = ssh::remote_project_dir(&cfg.remote.backup_path, &project_name);
        ssh::ensure_remote_dir_exists(
            &cfg.remote.ssh_key,
            &known_hosts,
            &cfg.remote.host,
            &cfg.remote.user,
            cfg.remote.port,
            &project_remote,
        )
        .await?;

        let link_dest = snapshots.first().map(|name| {
            crate::backup::rsync::absolute_remote_snapshot_path(
                &cfg.remote.backup_path,
                &project_name,
                name,
            )
        });

        let new_name = Utc::now().format("%Y-%m-%d_%H-%M-%S").to_string();
        let dest_folder =
            remote_snapshot_dest_folder(&cfg.remote.backup_path, &project_name, &new_name);

        sink.backup_progress_line(format!(
            "[backr] syncing project {project_name} -> {}@{}:{}",
            cfg.remote.user, cfg.remote.host, dest_folder
        ));

        rsync::rsync_backup_snapshot(
            sink.clone(),
            &cfg.remote.ssh_key,
            &known_hosts,
            &local_dir,
            link_dest.as_deref(),
            &cfg.remote.user,
            &cfg.remote.host,
            cfg.remote.port,
            &dest_folder,
        )
        .await?;

        let snapshot_count_after = snapshots.len() + 1;
        crate::project_snapshot_cache::record_backup_success(
            &cfg,
            &project_name,
            &new_name,
            snapshot_count_after,
        )?;
    }

    let now = Utc::now();
    {
        let mut last = state.last_backup_at.lock().await;
        *last = Some(now);
    }

    let mut updated = cfg.clone();
    updated.state.last_backup_at = Some(now);
    config::save_config(&updated)?;

    {
        let mut guard = state.config.lock().await;
        *guard = Some(updated);
    }

    sink.backup_progress_line("[backr] backup completed successfully".into());
    Ok(())
}

/// Runs the full backup pipeline for all projects (or one) with rsync progress events.
///
/// # Inputs
///
/// * `app` — global handle for emitting `backup://progress` lines.
/// * `state` — shared mutable configuration and progress flags.
/// * `project` — optional limiting project directory name.
///
/// # Returns
///
/// `Ok` when every targeted project sync completes; `Err` describing the first fatal failure.
pub async fn execute_backup_cycle(
    app: &AppHandle,
    state: &Arc<AppState>,
    project: Option<String>,
) -> Result<(), BackrError> {
    let sink: SharedProgress = Arc::new(AppEmitProgress::new(app.clone()));
    execute_backup_cycle_with_sink(sink, state, project).await?;
    let _ = crate::tray::update_tooltip(app, state).await;
    Ok(())
}

/// Spawns an asynchronous backup job and returns immediately (fire-and-forget semantics).
///
/// # Inputs
///
/// * `project` — optional directory name restricting the sync to a single project.
///
/// # Returns
///
/// `Ok(())` when the worker was scheduled; `Err` when another backup is active.
pub fn spawn_backup_job(
    app: &AppHandle,
    state: &Arc<AppState>,
    project: Option<String>,
) -> Result<(), String> {
    if state
        .in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("a backup is already in progress".into());
    }

    let app = app.clone();
    let state = Arc::clone(state);

    tauri::async_runtime::spawn(async move {
        let _clear = InProgressDrop {
            state: Arc::clone(&state),
        };
        let res = execute_backup_cycle(&app, &state, project).await;
        if let Err(err) = res {
            let _ = app.emit(
                rsync::BACKUP_PROGRESS_EVENT,
                format!("[backr] error: {err}"),
            );
        }
        drop(_clear);
        {
            let mut ap = state.active_project.lock().await;
            *ap = None;
        }
        let _ = crate::tray::update_tooltip(&app, &state).await;
    });

    Ok(())
}

/// Tauri command entrypoint delegating to [`spawn_backup_job`].
#[tauri::command]
pub async fn run_backup(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    project: Option<String>,
) -> Result<(), String> {
    // Validate the optional project name before dispatching — rejects traversal attempts.
    if let Some(ref p) = project {
        validate_remote_component(p).map_err(|e| format!("invalid project: {e}"))?;
    }
    spawn_backup_job(&app, state.inner(), project)
}

/// Runs one scheduled backup (all projects), skipping silently while another job holds the lock.
///
/// # Inputs
///
/// * `app` — global Tauri handle used to reach managed state and emit events.
///
/// # Returns
///
/// `Ok` when the tick completes or was skipped.
pub async fn run_scheduled_backup(app: AppHandle) -> Result<(), String> {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    if state
        .in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let app_handle = app.clone();
    let _clear = InProgressDrop {
        state: Arc::clone(&state),
    };
    let res = execute_backup_cycle(&app_handle, &state, None).await;
    if let Err(err) = res {
        tracing::warn!("scheduled backup failed: {err}");
        let _ = app_handle.emit(
            rsync::BACKUP_PROGRESS_EVENT,
            format!("[backr] scheduled backup error: {err}"),
        );
    }
    drop(_clear);
    {
        let mut ap = state.active_project.lock().await;
        *ap = None;
    }
    let _ = crate::tray::update_tooltip(&app, &state).await;
    Ok(())
}

/// Resolves either every child directory of the configured projects root or a single explicit project.
///
/// # Inputs
///
/// * `only` — optional folder name under `local.projects_path`.
///
/// # Returns
///
/// Sorted project directory names existing on disk.
fn resolve_targets(cfg: &Config, only: Option<&str>) -> Result<Vec<String>, BackrError> {
    let base = Path::new(&cfg.local.projects_path);
    if let Some(one) = only {
        let p = base.join(one);
        if !p.is_dir() {
            return Err(BackrError::Msg(format!(
                "project folder does not exist: {}",
                p.display()
            )));
        }
        return Ok(vec![one.to_string()]);
    }

    let mut out: Vec<String> = std::fs::read_dir(base)
        .map_err(BackrError::Io)?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    Ok(out)
}
