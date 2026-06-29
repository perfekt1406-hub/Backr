/*
 * scheduler.rs — Daemon-side backup trigger and scheduler wiring for backrd.
 *
 * This module provides:
 *   - `DaemonBackupTrigger` — implements `BackupTrigger` from `backr_core::scheduler`.
 *     When the periodic scheduler fires it spawns a Tokio task that runs the real rsync
 *     backup via `ipc::handlers::execute_backup_cycle_with_sink` and broadcasts progress
 *     to connected GUI clients via `IpcBroadcastSink`.  After each successful backup the
 *     tray label is refreshed (Linux only; no-op on other platforms).
 *   - `start_scheduler_if_configured` — reads the current config from `DaemonState` and
 *     calls `restart_scheduler` if a config is present.  Called once at daemon startup
 *     and again whenever config changes (via `save_config` handler).
 *
 * No Tauri types are used anywhere in this file.
 */

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tokio::sync::broadcast;

use backr_core::progress_sink::SharedProgress;
use backr_core::scheduler::{restart_scheduler, BackupTrigger};

use crate::daemon_state::DaemonState;
use crate::event_sink::IpcBroadcastSink;
use crate::ipc::handlers::execute_backup_cycle_with_sink;
use crate::ipc::protocol::IpcEvent;

// ---------------------------------------------------------------------------
// DaemonBackupTrigger
// ---------------------------------------------------------------------------

/// Implements `BackupTrigger` for the backrd daemon.
///
/// Holds a reference to shared daemon state and a broadcast sender so that
/// triggered backup tasks can report progress to connected GUI clients.
/// `Arc<DaemonBackupTrigger>` is passed to `restart_scheduler`; the scheduler
/// calls `trigger_backup` on each periodic tick.
pub struct DaemonBackupTrigger {
    /// Shared daemon state (config, in-progress flag, last-backup timestamp).
    pub(crate) state: Arc<DaemonState>,
    /// Sender side of the IPC event broadcast channel.
    pub(crate) event_tx: broadcast::Sender<IpcEvent>,
}

impl DaemonBackupTrigger {
    /// Constructs a new trigger.
    ///
    /// # Parameters
    /// - `state`    — Shared `Arc<DaemonState>` cloned from the daemon's main state.
    /// - `event_tx` — Broadcast sender for pushing `IpcEvent` messages to all GUI clients.
    pub fn new(state: Arc<DaemonState>, event_tx: broadcast::Sender<IpcEvent>) -> Self {
        Self { state, event_tx }
    }
}

impl BackupTrigger for DaemonBackupTrigger {
    /// Spawns an async task to execute a scheduled backup of all projects.
    ///
    /// Silently skips the tick when another backup job already holds the `in_progress`
    /// flag (mirrors the behaviour of `backup_cmd.rs::run_scheduled_backup`).
    /// Progress lines are broadcast to all connected clients via `IpcBroadcastSink`.
    /// On Linux, the system tray label is updated after each successful backup.
    ///
    /// Non-blocking: returns immediately after `tokio::spawn`.
    fn trigger_backup(&self) {
        let state = Arc::clone(&self.state);
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            // Skip silently when another backup is already running.
            if state
                .in_progress
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                tracing::info!("scheduler: backup tick skipped — another job is active");
                return;
            }

            tracing::info!("scheduler: backup triggered");

            // Ensure in_progress is cleared when the task exits, even on error.
            struct InProgressDrop(Arc<DaemonState>);
            impl Drop for InProgressDrop {
                fn drop(&mut self) {
                    self.0.in_progress.store(false, Ordering::SeqCst);
                }
            }
            let _clear = InProgressDrop(Arc::clone(&state));

            /* IpcBroadcastSink implements ProgressSink — broadcasts rsync lines to all connected GUI clients. */
            let sink: SharedProgress = Arc::new(IpcBroadcastSink::new(tx.clone()));

            /* execute_backup_cycle_with_sink runs the full backup pipeline with rsync progress events. */
            let res = execute_backup_cycle_with_sink(sink, &state, None).await;
            if let Err(err) = res {
                tracing::warn!("scheduler: scheduled backup failed: {err}");
                let _ = tx.send(IpcEvent {
                    event: "backup_progress".into(),
                    data: serde_json::json!(format!("[backr] scheduled backup error: {err}")),
                });
            } else {
                // Refresh the system tray tooltip with the new last-backup time.
                // On non-Linux platforms this call compiles to a no-op.
                #[cfg(target_os = "linux")]
                crate::tray::update_label(&state);
            }

            drop(_clear);
            {
                let mut ap = state.active_project.lock().await;
                *ap = None;
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Startup helper
// ---------------------------------------------------------------------------

/// Starts (or restarts) the scheduler using the config currently loaded in `state`.
///
/// Reads the config under a short-lived async lock (released before awaiting
/// `restart_scheduler`) to avoid holding the mutex across an `.await` point.
/// If no config is loaded the function returns without error — the scheduler
/// remains inactive until config is written and this function is called again.
///
/// # Parameters
/// - `state`    — Shared daemon state; used to read the current config and to
///                build the `DaemonBackupTrigger`.
/// - `event_tx` — Broadcast sender wired into the `DaemonBackupTrigger` so backup
///                tasks can push progress events to connected clients.
pub async fn start_scheduler_if_configured(
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
) {
    // Read the config under a brief lock and release before awaiting.
    let cfg_snapshot = {
        let guard = state.config.lock().await;
        guard.clone()
    };

    let sched = Arc::clone(&state.scheduler);
    let trigger: Arc<DaemonBackupTrigger> =
        Arc::new(DaemonBackupTrigger::new(Arc::clone(&state), event_tx));

    // `restart_scheduler` cancels any existing scheduler and starts a fresh one.
    // When `cfg_snapshot` is `None` it just cancels any running scheduler.
    if let Err(e) = restart_scheduler(cfg_snapshot.as_ref(), &sched, trigger).await {
        tracing::error!("failed to start scheduler: {e}");
    }
}
