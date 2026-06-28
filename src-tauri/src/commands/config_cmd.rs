/*
 * Configuration-related Tauri commands: load, save, and SSH connectivity checks.
 * Saving restarts the periodic backup scheduler so interval changes take effect immediately.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use tauri::{AppHandle, State};

use crate::config::{self, Config};
use crate::error::{BackrCommandError, BackrError};
use crate::scheduler;
use crate::state::AppState;
use std::sync::Arc;

/// Returns the persisted configuration, or `None` when `config.toml` is absent.
///
/// # Returns
///
/// `Ok(Some(cfg))` when configured; `Ok(None)` for first-launch scenarios.
#[tauri::command]
pub async fn get_config(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<Config>, BackrCommandError> {
    let guard = state.config.lock().await;
    Ok(guard.clone())
}

/// Replaces in-memory configuration, writes `config.toml`, and restarts the scheduler loop.
///
/// # Inputs
///
/// * `next` — full configuration snapshot provided by the UI setup wizard.
#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    next: Config,
) -> Result<(), BackrCommandError> {
    /* config::save_config writes config.toml atomically (write + rename). */
    config::save_config(&next).map_err(BackrCommandError::from)?;
    {
        let mut guard = state.config.lock().await;
        *guard = Some(next);
    }
    /* scheduler::restart_scheduler stops any running scheduler and starts a fresh one. */
    scheduler::restart_scheduler(&app, state.inner())
        .await
        .map_err(BackrCommandError::from)?;
    Ok(())
}

/// Verifies SSH key-based authentication using a lightweight remote `echo` probe.
///
/// # Inputs
///
/// * `host`     — SSH hostname or address.
/// * `user`     — remote login user.
/// * `key_path` — path to the SSH private key (tilde-expanded).
/// * `ssh_port` — optional SSH TCP port (`22` default when omitted).
#[tauri::command]
pub async fn test_connection(
    host: String,
    user: String,
    key_path: String,
    ssh_port: Option<u16>,
) -> Result<(), BackrCommandError> {
    /* config::expand_path_str resolves `~` and env vars in the key path. */
    let expanded =
        config::expand_path_str(&key_path).map_err(|e: BackrError| BackrCommandError::from(e))?;
    let port = ssh_port.unwrap_or(22);
    /* ssh::test_connection runs a remote `echo` to verify key-based auth; returns BackrError. */
    crate::backup::ssh::test_connection(&host, &user, &expanded, port)
        .await
        .map_err(BackrCommandError::from)
}
