/*
 * Process-wide mutable state shared between Tauri commands and tray updates.
 *
 * In the daemon-GUI split model the daemon owns all backup state (scheduler, in-progress
 * flag, active project, config persistence).  AppState is simplified: it retains only the
 * fields needed by the Tauri GUI shell — pairing runtime (host-side), daemon error from
 * startup, and backward-compatible scheduler fields that may still be referenced by
 * residual code paths.
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
    ///
    /// Retained for backward compatibility with `require_config()` and tray helpers that
    /// read the schedule interval; the authoritative copy lives in the daemon.
    pub config: Mutex<Option<Config>>,

    /// Set while an on-demand or scheduled backup worker is active.
    ///
    /// Retained for backward compatibility; the daemon owns the true in-progress guard.
    pub in_progress: AtomicBool,

    /// Project directory name currently being synced (if any).
    ///
    /// Retained for backward compatibility with residual tray update paths.
    pub active_project: Mutex<Option<String>>,

    /// Last successful backup instant mirrored from the daemon response.
    ///
    /// Retained for tray tooltip updates.
    pub last_backup_at: Mutex<Option<DateTime<Utc>>>,

    /// Handle for the background scheduler task (daemon now owns the real scheduler;
    /// this field is kept to avoid breaking residual references in scheduler.rs).
    pub scheduler_handle: Mutex<Option<JoinHandle<()>>>,

    /// Token used to stop the current scheduler loop (kept for residual references).
    pub scheduler_cancel: Mutex<Option<CancellationToken>>,

    /// Active one-tap pairing window (host side), or `None` when not pairing.
    ///
    /// Retained because pairing_cmd still needs AppState for the host-side pairing state
    /// when the daemon is not yet handling pairing.
    pub pairing: Mutex<Option<PairingSession>>,

    /// Live mDNS + listener resources while a pairing window is open.
    pub pairing_runtime: Mutex<Option<PairingRuntime>>,

    /// Daemon connectivity error recorded during startup.
    ///
    /// `Some(message)` when the daemon was unreachable and could not be spawned.
    /// Surfaced to the frontend via the `get_daemon_error` Tauri command so it can show
    /// a clear error screen instead of silently failing on the first `invoke()`.
    pub daemon_error: Mutex<Option<String>>,
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
    /// Constructs default state with no configuration loaded and no active scheduler or daemon error.
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
            daemon_error: Mutex::new(None),
        }
    }
}
