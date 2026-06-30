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

    /// Loads `config.toml` from disk into [`config`](Self::config) when present.
    ///
    /// Must be called once at daemon startup: `DaemonState::new()` starts with
    /// `config = None`, and the only other writers are the `save_config` handler and
    /// the post-backup update. Without this hydration a configured client's daemon
    /// comes up unconfigured after every (re)start — the scheduler never starts and
    /// every config-reading command (`get_config`, `get_backup_status`, `list_*`,
    /// `run_backup`) returns `NotConfigured` until the GUI next calls `save_config`.
    ///
    /// A missing config file is treated as first-launch (left as `None`); a read or
    /// parse error is logged and also left as `None` so the daemon still serves setup.
    pub async fn hydrate_config_from_disk(&self) {
        /* config::load_config returns Ok(None) when config.toml is absent, Ok(Some) when parsed. */
        match backr_core::config::load_config() {
            Ok(Some(cfg)) => {
                *self.config.lock().await = Some(cfg);
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("failed to load configuration at startup: {e}");
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use backr_core::config::{
        save_config, Config, LocalConfig, RemoteConfig, ScheduleConfig, StateConfig, UpdateConfig,
        CONFIG_VERSION,
    };

    /// Regression test for the startup-hydration bug: a daemon coming up with an
    /// existing `config.toml` on disk must load it into `state.config`. Otherwise the
    /// scheduler never starts and every config-reading command returns `NotConfigured`
    /// until the GUI happens to re-save. Drives `config_path()` (via `dirs::config_dir`)
    /// at a temp `XDG_CONFIG_HOME` so the real on-disk path is exercised in isolation.
    #[tokio::test]
    async fn hydrate_loads_existing_config_from_disk() {
        let tmp = std::env::temp_dir().join(format!(
            "backrd-hydrate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", &tmp);

        // No config on disk yet → hydration leaves the daemon unconfigured.
        let state = DaemonState::new();
        state.hydrate_config_from_disk().await;
        assert!(
            state.config.lock().await.is_none(),
            "absent config.toml should leave state unconfigured"
        );

        // Persist a config, then a freshly-constructed daemon must pick it up at startup.
        let cfg = Config {
            version: CONFIG_VERSION,
            remote: RemoteConfig {
                host: "nas.local".into(),
                user: "backr".into(),
                ssh_key: "/tmp/backr-test-key".into(),
                port: 22,
                backup_path: "/srv/backups".into(),
            },
            local: LocalConfig {
                projects_path: "/tmp/backr-test-projects".into(),
            },
            schedule: ScheduleConfig { interval_hours: 6 },
            state: StateConfig {
                last_backup_at: None,
            },
            update: UpdateConfig::default(),
        };
        save_config(&cfg).unwrap();

        let state = DaemonState::new();
        state.hydrate_config_from_disk().await;
        let loaded = state.config.lock().await.clone();
        assert_eq!(
            loaded.as_ref(),
            Some(&cfg),
            "daemon startup must hydrate the existing config from disk"
        );

        // Restore env so this test cannot leak into others in this binary.
        match prev {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
