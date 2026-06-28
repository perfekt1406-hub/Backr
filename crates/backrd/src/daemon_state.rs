/*
 * daemon_state.rs — shared mutable state for the backrd daemon.
 *
 * `DaemonState` is the single owner of all runtime state: loaded configuration,
 * backup progress flag, last-backup timestamp, scheduler handles, and the active
 * pairing session. It is wrapped in `Arc` and cloned into each IPC connection
 * handler and future scheduler tasks.
 *
 * No Tauri types appear here; this module is intentionally framework-agnostic.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use backr_core::config::Config;
use backr_core::pairing::code::PairingSession;
use backr_core::scheduler::SchedulerState;

/// All shared daemon runtime state, accessed through `Arc<DaemonState>`.
///
/// Each field is independently locked so that concurrent IPC connections do not
/// block each other unnecessarily.
pub struct DaemonState {
    /// Current loaded configuration, or `None` if not yet read from disk.
    pub config: Mutex<Option<Config>>,

    /// `true` while a backup job is actively running; guards against concurrent runs.
    pub in_progress: AtomicBool,

    /// Wall-clock time of the most recently completed backup, if any.
    pub last_backup_at: Mutex<Option<DateTime<Utc>>>,

    /// Tokio task handle and cancellation token for the periodic backup scheduler.
    /// Stored as `Arc` so it can be passed directly to `restart_scheduler` without
    /// requiring an additional `Arc` wrapper at the call site.
    pub scheduler: Arc<SchedulerState>,

    /// Active pairing session (host side), or `None` when not pairing.
    pub pairing: Mutex<Option<PairingSession>>,
}

impl DaemonState {
    /// Constructs empty daemon state with no running scheduler and no active session.
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
            in_progress: AtomicBool::new(false),
            last_backup_at: Mutex::new(None),
            scheduler: Arc::new(SchedulerState::new()),
            pairing: Mutex::new(None),
        }
    }
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}
