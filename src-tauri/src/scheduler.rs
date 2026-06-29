/*
 * Tauri-side scheduler wiring.
 *
 * The platform-independent scheduler loop and `BackupTrigger` trait live in
 * `backr_core::scheduler`.  This file bridges the gap: it provides a
 * `TauriBackupTrigger` that wraps `AppHandle` and calls `spawn_backup_job`, and
 * a `restart_scheduler` wrapper that extracts the config from `AppState` and
 * delegates to `backr_core::scheduler::restart_scheduler`.
 */

use std::sync::Arc;

use tauri::AppHandle;

use backr_core::scheduler::{BackupTrigger, SchedulerState, restart_scheduler as core_restart};

use crate::commands::backup_cmd::spawn_backup_job;
use crate::state::AppState;

/// Implements [`BackupTrigger`] by calling [`spawn_backup_job`] on the Tauri runtime.
///
/// Held by the `backr_core` scheduler loop as an `Arc<dyn BackupTrigger>`.
struct TauriBackupTrigger {
    /// Tauri application handle used to access managed state and emit events.
    app: AppHandle,
    /// Shared application state required by `spawn_backup_job`.
    state: Arc<AppState>,
}

impl BackupTrigger for TauriBackupTrigger {
    /// Fires a backup job via `spawn_backup_job` (all projects, no project filter).
    ///
    /// `spawn_backup_job` returns `Err` only when another job is already running —
    /// the scheduler silently skips that tick, which is correct behaviour.
    fn trigger_backup(&self) {
        if let Err(err) = spawn_backup_job(&self.app, &self.state, None) {
            tracing::warn!("scheduler tick skipped: {err:?}");
        }
    }
}

/// Stops any existing scheduler, then (if configured) starts a new sleeping loop.
///
/// Reads config from `AppState`, builds a `TauriBackupTrigger`, and delegates to
/// `backr_core::scheduler::restart_scheduler`.
///
/// # Inputs
///
/// * `app`   — global handle forwarded to scheduled backup jobs.
/// * `state` — shared [`AppState`] carrying join handles, tokens, and config.
///
/// # Returns
///
/// `Ok` after the replacement completes; errors are surfaced as plain strings for setup code.
pub async fn restart_scheduler(app: &AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    // Extract the scheduler handles from AppState so we can pass a SchedulerState to core.
    // We share the same tokio Mutex slots that AppState already has.
    let sched = Arc::new(SchedulerState::new());

    // Drain the existing handles from AppState into the SchedulerState before restarting.
    {
        let mut old_cancel = state.scheduler_cancel.lock().await;
        if let Some(token) = old_cancel.take() {
            token.cancel();
        }
    }
    {
        let mut old_handle = state.scheduler_handle.lock().await;
        if let Some(h) = old_handle.take() {
            h.abort();
        }
    }

    let cfg = state.config.lock().await.clone();

    let trigger: Arc<dyn BackupTrigger> = Arc::new(TauriBackupTrigger {
        app: app.clone(),
        state: Arc::clone(state),
    });

    // Delegate to the core scheduler implementation.
    core_restart(cfg.as_ref(), &sched, trigger).await?;

    // Move new handles back into AppState so the rest of the app can still manage them.
    {
        let mut cancel_slot = state.scheduler_cancel.lock().await;
        let mut sched_cancel = sched.cancel.lock().await;
        *cancel_slot = sched_cancel.take();
    }
    {
        let mut handle_slot = state.scheduler_handle.lock().await;
        let mut sched_handle = sched.handle.lock().await;
        *handle_slot = sched_handle.take();
    }

    Ok(())
}
