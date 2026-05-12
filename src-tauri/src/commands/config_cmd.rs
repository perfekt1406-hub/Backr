/*
 * Configuration-related Tauri commands: load, save, and SSH connectivity checks.
 * Saving restarts the periodic backup scheduler so interval changes take effect immediately.
 */

use tauri::{AppHandle, State};

use crate::config::{self, Config};
use crate::error::BackrError;
use crate::scheduler;
use crate::state::AppState;
use std::sync::Arc;

/// Returns the persisted configuration, or `None` when `config.toml` is absent.
///
/// # Returns
///
/// `Ok(Some(cfg))` when configured; `Ok(None)` for first-launch scenarios.
#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Option<Config>, String> {
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
) -> Result<(), String> {
    config::save_config(&next).map_err(|e: BackrError| e.to_string())?;
    {
        let mut guard = state.config.lock().await;
        *guard = Some(next);
    }
    scheduler::restart_scheduler(&app, state.inner()).await?;
    Ok(())
}

/// Verifies SSH key-based authentication using a lightweight remote `echo` probe.
///
/// # Inputs
///
/// * `host` — SSH hostname or address.
/// * `user` — remote login user.
/// * `ssh_port` — optional SSH TCP port (`22` default when omitted).
#[tauri::command]
pub async fn test_connection(
    host: String,
    user: String,
    key_path: String,
    ssh_port: Option<u16>,
) -> Result<(), String> {
    let expanded = config::expand_path_str(&key_path).map_err(|e: BackrError| e.to_string())?;
    let port = ssh_port.unwrap_or(22);
    crate::backup::ssh::test_connection(&host, &user, &expanded, port)
        .await
        .map_err(|e: BackrError| e.to_string())
}
