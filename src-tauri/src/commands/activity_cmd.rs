/*
 * Lightweight backup activity samples for dashboard timelines.
 *
 * Thin IPC proxy: delegates to the backrd daemon and deserializes the response.
 * The frontend `invoke()` call contract (function name, param types, return type) is
 * preserved exactly.
 *
 * Returns `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::BackrCommandError;
use crate::state::AppState;

/// One marker on the backup activity strip (Unix seconds + stable label key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityPoint {
    /// Seconds since UNIX epoch (UTC).
    pub ts_unix: i64,
    /// Internal discriminator for UI badges (`backup_complete`, etc.).
    pub label: String,
}

/// Returns recent backup completion markers from the daemon.
///
/// # Inputs
///
/// * `state` — managed [`AppState`] (unused by the proxy; kept for signature compatibility).
///
/// # Returns
///
/// Empty when unconfigured or no successful backup yet; otherwise one point per stored completion.
#[tauri::command]
pub async fn get_activity_series(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<ActivityPoint>, BackrCommandError> {
    let v = crate::ipc_client::send("get_activity_series", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize activity series: {e}"))
    })
}
