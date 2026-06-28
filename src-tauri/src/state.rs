/*
 * Process-wide mutable state shared between Tauri commands, the scheduler, and backups.
 * Holds loaded configuration, backup concurrency guards, and scheduler lifecycle handles.
 */

use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::error::BackrCommandError;
use crate::pairing::code::PairingSession;
use crate::pairing::PairingRuntime;

/// Shared application state injected into the Tauri runtime via `tauri::Manager::manage`.
pub struct AppState {
    /// Loaded configuration from disk, or `None` before first successful save.
    pub config: Mutex<Option<Config>>,
    /// Set while an on-demand or scheduled backup worker is active.
    pub in_progress: AtomicBool,
    /// Project directory name currently being synced (if any), excluding multi-project “all” runs.
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
