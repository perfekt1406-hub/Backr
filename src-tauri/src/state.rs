/*
 * Process-wide mutable state shared between Tauri commands, the scheduler, and backups.
 * Holds loaded configuration, backup concurrency guards, and scheduler lifecycle handles.
 *
 * Business-logic types (Config, PairingSession, PairingRuntime) are imported from
 * `backr_core`; only the Tauri-coupled scheduler handles and the `AppState` struct
 * itself live here.
 */

use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use backr_core::config::Config;
use backr_core::error::BackrCommandError;
use backr_core::pairing::code::PairingSession;
use backr_core::pairing::{PairingRuntime, listener::{PairingStateAccess, PairRequest, HostPairInfo, PairRejection, process_pair}};
use backr_core::host_trust::host_append_authorized_pubkey_impl;

/// Shared application state injected into the Tauri runtime via `tauri::Manager::manage`.
pub struct AppState {
    /// Loaded configuration from disk, or `None` before first successful save.
    pub config: Mutex<Option<Config>>,
    /// Set while an on-demand or scheduled backup worker is active.
    pub in_progress: AtomicBool,
    /// Project directory name currently being synced (if any), excluding multi-project "all" runs.
    pub active_project: Mutex<Option<String>>,
    /// Last successful backup instant (also mirrored into config file when persisted).
    pub last_backup_at: Mutex<Option<DateTime<Utc>>>,
    /// Handle for the background scheduler task (replaced when configuration restarts it).
    pub scheduler_handle: Mutex<Option<JoinHandle<()>>>,
    /// Token used to stop the current scheduler loop when configuration changes.
    pub scheduler_cancel: Mutex<Option<CancellationToken>>,
    /// Active one-tap pairing window (host side), or `None` when not pairing.
    pub pairing: Mutex<Option<PairingSession>>,
    /// Live mDNS + listener resources while a pairing window is open.
    pub pairing_runtime: Mutex<Option<PairingRuntime>>,
}

impl AppState {
    /// Returns a clone of the loaded [`Config`], or a typed `NotConfigured` error when the
    /// application has not yet been set up.
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

impl Default for AppState {
    /// Constructs default state with no configuration loaded and no active scheduler.
    fn default() -> Self {
        Self {
            config: Mutex::new(None),
            in_progress: AtomicBool::new(false),
            active_project: Mutex::new(None),
            last_backup_at: Mutex::new(None),
            scheduler_handle: Mutex::new(None),
            scheduler_cancel: Mutex::new(None),
            pairing: Mutex::new(None),
            pairing_runtime: Mutex::new(None),
        }
    }
}

/// Implements `PairingStateAccess` for `AppState` so `backr_core::pairing::listener::serve`
/// can operate on the Tauri app state without depending on Tauri types.
impl PairingStateAccess for AppState {
    /// Processes one pair request by locking the session and calling `process_pair`.
    ///
    /// # Inputs
    ///
    /// * `req`  — the pair request from the laptop.
    /// * `host` — host info to return on success.
    ///
    /// # Returns
    ///
    /// `Ok(HostPairInfo)` on success, `Err(Some(rej))` on rejection, `Err(None)` when
    /// there is no active session.
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
        process_pair(session, req, host, |line| {
            let r = host_append_authorized_pubkey_impl(line.to_string())
                .map_err(|e| e)?;
            // appended=false + skipped_duplicate=false means the host process doesn't own
            // authorized_keys — the sudo fallback path returned Ok but wrote nothing.
            if r.appended || r.skipped_duplicate {
                Ok(())
            } else {
                Err("host cannot write authorized_keys — run the sudo snippet from the Trust keys UI".to_string())
            }
        }).map_err(Some)
    }

    /// Clears the active pairing session slot.
    fn clear_pairing_session(&self) {
        *self.pairing.blocking_lock() = None;
    }

    /// Takes the `PairingRuntime` out of its slot and returns it (sets slot to `None`).
    ///
    /// # Returns
    ///
    /// `Some(PairingRuntime)` if a window was open, `None` if already torn down.
    fn take_pairing_runtime(&self) -> Option<PairingRuntime> {
        self.pairing_runtime.blocking_lock().take()
    }
}
