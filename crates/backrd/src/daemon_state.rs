/*
 * daemon_state.rs — shared mutable state for the backrd daemon.
 *
 * `DaemonState` is the single owner of all runtime state: loaded configuration,
 * backup progress flag, last-backup timestamp, scheduler handles, active pairing
 * session, pairing runtime, and the active project name during a backup run.
 * It is wrapped in `Arc` and cloned into each IPC connection handler and scheduler
 * task.
 *
 * Implements `PairingStateAccess` from `backr_core::pairing::listener` so the
 * host-side pairing serve loop can operate on daemon state without depending on
 * Tauri types.
 *
 * No Tauri types appear here; this module is intentionally framework-agnostic.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;

use backr_core::config::Config;
use backr_core::error::BackrCommandError;
use backr_core::host_trust::host_append_authorized_pubkey_impl;
use backr_core::pairing::code::PairingSession;
use backr_core::pairing::listener::{
    process_pair, HostPairInfo, PairRejection, PairRequest, PairingStateAccess,
};
use backr_core::pairing::PairingRuntime;
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

    /// Project directory name currently being synced (if any), excluding multi-project runs.
    pub active_project: Mutex<Option<String>>,

    /// Wall-clock time of the most recently completed backup, if any.
    pub last_backup_at: Mutex<Option<DateTime<Utc>>>,

    /// Tokio task handle and cancellation token for the periodic backup scheduler.
    /// Stored as `Arc` so it can be passed directly to `restart_scheduler` without
    /// requiring an additional `Arc` wrapper at the call site.
    pub scheduler: Arc<SchedulerState>,

    /// Active pairing session (host side), or `None` when not pairing.
    pub pairing: Mutex<Option<PairingSession>>,

    /// Live mDNS + listener resources while a pairing window is open.
    pub pairing_runtime: Mutex<Option<PairingRuntime>>,
}

impl DaemonState {
    /// Constructs empty daemon state with no running scheduler and no active session.
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
            in_progress: AtomicBool::new(false),
            active_project: Mutex::new(None),
            last_backup_at: Mutex::new(None),
            scheduler: Arc::new(SchedulerState::new()),
            pairing: Mutex::new(None),
            pairing_runtime: Mutex::new(None),
        }
    }

    /// Returns a clone of the loaded [`Config`], or a typed `NotConfigured` error when the
    /// daemon has not yet been configured.
    ///
    /// # Returns
    ///
    /// `Ok(Config)` when configuration has been saved; `Err(BackrCommandError::NotConfigured)`
    /// otherwise.
    pub async fn require_config(&self) -> Result<Config, BackrCommandError> {
        self.config
            .lock()
            .await
            .clone()
            .ok_or_else(BackrCommandError::not_configured)
    }
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

/// Implements `PairingStateAccess` for `DaemonState` so `backr_core::pairing::listener::serve`
/// can operate on daemon state without depending on Tauri types.
///
/// All methods use `blocking_lock` because `serve` runs on a dedicated OS thread
/// outside the Tokio executor.
impl PairingStateAccess for DaemonState {
    /// Processes one pair request by locking the pairing session and calling `process_pair`.
    ///
    /// # Inputs
    ///
    /// * `req`  — the pair request from the connecting laptop.
    /// * `host` — host info to return to the laptop on success.
    ///
    /// # Returns
    ///
    /// `Ok(HostPairInfo)` on success, `Err(Some(rej))` on rejection,
    /// `Err(None)` when there is no active session.
    fn process_pair_request(
        &self,
        req: &PairRequest,
        host: &HostPairInfo,
    ) -> Result<HostPairInfo, Option<PairRejection>> {
        // Use blocking_lock because serve runs on a dedicated OS thread (not a Tokio worker).
        let mut guard = self.pairing.blocking_lock();
        let Some(session) = guard.as_mut() else {
            return Err(None);
        };
        /* process_pair validates the pubkey, consumes the code, appends to authorized_keys, and returns HostPairInfo. */
        process_pair(session, req, host, |line| {
            /* host_append_authorized_pubkey_impl validates and appends one pubkey line to authorized_keys. */
            let r = host_append_authorized_pubkey_impl(line.to_string())
                .map_err(|e| e)?;
            // appended=false + skipped_duplicate=false means the daemon process cannot own
            // authorized_keys — the sudo fallback path returned Ok but wrote nothing.
            if r.appended || r.skipped_duplicate {
                Ok(())
            } else {
                Err(
                    "daemon cannot write authorized_keys — run the sudo snippet from the Trust keys UI"
                        .to_string(),
                )
            }
        })
        .map_err(Some)
    }

    /// Clears the active pairing session slot (sets it to `None`).
    fn clear_pairing_session(&self) {
        *self.pairing.blocking_lock() = None;
    }

    /// Takes the active `PairingRuntime` out of its slot and returns it (sets slot to `None`).
    ///
    /// # Returns
    ///
    /// `Some(PairingRuntime)` if a window was open, `None` if already torn down.
    fn take_pairing_runtime(&self) -> Option<PairingRuntime> {
        self.pairing_runtime.blocking_lock().take()
    }
}
