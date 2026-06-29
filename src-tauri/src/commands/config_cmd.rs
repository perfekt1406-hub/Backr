/*
 * Configuration-related Tauri commands: load, save, and SSH connectivity checks.
 *
 * All commands are thin IPC proxies that delegate to the backrd daemon over a Unix
 * domain socket.  The function signatures are kept identical to preserve the
 * frontend `invoke()` call contract.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::config::Config;
use crate::error::BackrCommandError;
use crate::state::AppState;

/// Returns the persisted configuration from the daemon, or `None` when `config.toml` is absent.
///
/// # Returns
///
/// `Ok(Some(cfg))` when configured; `Ok(None)` for first-launch scenarios.
#[tauri::command]
pub async fn get_config(
    _state: State<'_, Arc<AppState>>,
) -> Result<Option<Config>, BackrCommandError> {
    let v = crate::ipc_client::send("get_config", serde_json::json!({})).await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::config(format!("failed to deserialize config: {e}")))
}

/// Sends the new configuration to the daemon, which writes `config.toml` and restarts its scheduler.
///
/// # Inputs
///
/// * `next` — full configuration snapshot provided by the UI setup wizard.
#[tauri::command]
pub async fn save_config(
    _app: AppHandle,
    _state: State<'_, Arc<AppState>>,
    next: Config,
) -> Result<(), BackrCommandError> {
    crate::ipc_client::send("save_config", serde_json::json!({ "config": next })).await?;
    Ok(())
}

/// Verifies SSH key-based authentication using a lightweight remote `echo` probe via the daemon.
///
/// # Inputs
///
/// * `host`     — SSH hostname or address.
/// * `user`     — remote login user.
/// * `key_path` — path to the SSH private key (tilde-expanded by the daemon).
/// * `ssh_port` — optional SSH TCP port (`22` default when omitted).
#[tauri::command]
pub async fn test_connection(
    host: String,
    user: String,
    key_path: String,
    ssh_port: Option<u16>,
) -> Result<(), BackrCommandError> {
    crate::ipc_client::send(
        "test_connection",
        serde_json::json!({
            "host": host,
            "user": user,
            "key_path": key_path,
            "ssh_port": ssh_port,
        }),
    )
    .await?;
    Ok(())
}
