/*
 * scheduler.rs — Daemon-side backup trigger and scheduler wiring for backrd.
 *
 * This module provides:
 *   - `DaemonBackupTrigger` — implements `BackupTrigger` from `backr_core::scheduler`.
 *     When the periodic scheduler fires it spawns a Tokio task that (in U5) will run the
 *     real rsync backup.  For now (U4) the task logs a stub message and constructs an
 *     `IpcBroadcastSink` so the plumbing is exercisable before the real backup exists.
 *   - `start_scheduler_if_configured` — reads the current config from `DaemonState` and
 *     calls `restart_scheduler` if a config is present.  Called once at daemon startup
 *     and again whenever config changes (U5).
 *
 * No Tauri types are used anywhere in this file.
 */

use std::sync::Arc;

use tokio::sync::broadcast;

use backr_core::scheduler::{restart_scheduler, BackupTrigger};

use crate::daemon_state::DaemonState;
use crate::event_sink::IpcBroadcastSink;
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
    state: Arc<DaemonState>,
    /// Sender side of the IPC event broadcast channel.
    event_tx: broadcast::Sender<IpcEvent>,
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
    /// Spawns an async task to execute a scheduled backup.
    ///
    /// The spawned task constructs an `IpcBroadcastSink` so that U5 can drop in
    /// the real rsync call without needing to change any wiring.  For now (U4)
    /// the task only logs a stub message and drops the sink.
    ///
    /// Non-blocking: returns immediately after `tokio::spawn`.
    fn trigger_backup(&self) {
        let state = Arc::clone(&self.state);
        let tx = self.event_tx.clone();

        tokio::spawn(async move {
            // U5 will replace this stub with the real rsync backup logic.
            // The IpcBroadcastSink is constructed here so U5 can call
            // `sink.backup_progress_line(...)` without additional wiring.
            tracing::info!("scheduler: backup triggered (stub — real logic added in U5)");

            let sink = IpcBroadcastSink::new(tx);

            // Suppress unused-variable warnings — both will be used in U5.
            drop(state);
            drop(sink);
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
