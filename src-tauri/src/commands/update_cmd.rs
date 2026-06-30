/*
 * Self-update Tauri commands.
 *
 * Thin IPC proxies: forward to the backrd daemon, which owns the update engine.
 * apply_update returns immediately — the daemon launches an out-of-process worker
 * (KTD4) that restarts the daemon, so the GUI just sees the restart.
 *
 * Returns `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::BackrCommandError;
use crate::state::AppState;

/// Current-vs-latest version summary from the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
}

/// Persisted update preference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    pub auto_update: bool,
}

/// Reports the running version and whether a newer release is available.
#[tauri::command]
pub async fn get_update_status(
    _state: State<'_, Arc<AppState>>,
) -> Result<UpdateStatus, BackrCommandError> {
    let v = crate::ipc_client::send("get_update_status", serde_json::json!({})).await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::config(format!("failed to deserialize update status: {e}")))
}

/// Returns whether automatic updates are enabled.
#[tauri::command]
pub async fn get_update_settings(
    _state: State<'_, Arc<AppState>>,
) -> Result<UpdateSettings, BackrCommandError> {
    let v = crate::ipc_client::send("get_update_settings", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize update settings: {e}"))
    })
}

/// Enables or disables automatic updates.
#[tauri::command]
pub async fn set_update_settings(
    auto_update: bool,
    _state: State<'_, Arc<AppState>>,
) -> Result<UpdateSettings, BackrCommandError> {
    let v = crate::ipc_client::send(
        "set_update_settings",
        serde_json::json!({ "auto_update": auto_update }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize update settings: {e}"))
    })
}

/// Asks the daemon to apply the latest update (launches the out-of-process worker).
#[tauri::command]
pub async fn apply_update(
    _state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, BackrCommandError> {
    crate::ipc_client::send("apply_update", serde_json::json!({})).await
}
