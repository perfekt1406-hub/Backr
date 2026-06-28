/*
 * Lightweight backup activity samples for dashboard timelines.
 * Surfaces persisted cadence metadata without maintaining a separate event database.
 *
 * Returns `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::error::BackrCommandError;
use crate::state::AppState;

/// One marker on the backup activity strip (Unix seconds + stable label key).
#[derive(Debug, Clone, Serialize)]
pub struct ActivityPoint {
    /// Seconds since UNIX epoch (UTC).
    pub ts_unix: i64,
    /// Internal discriminator for UI badges (`backup_complete`, etc.).
    pub label: String,
}

/// Returns recent backup completion markers derived from persisted `[state]`.
///
/// # Inputs
///
/// * `state` — managed [`AppState`] holding the loaded optional [`crate::config::Config`].
///
/// # Returns
///
/// Empty when unconfigured or no successful backup yet; otherwise one point per stored completion.
#[tauri::command]
pub async fn get_activity_series(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ActivityPoint>, BackrCommandError> {
    let cfg_opt = state.config.lock().await.clone();
    /* Return an empty series gracefully when the app is not yet configured. */
    let Some(cfg) = cfg_opt else {
        return Ok(Vec::new());
    };
    let mut pts = Vec::new();
    if let Some(last) = cfg.state.last_backup_at {
        pts.push(ActivityPoint {
            ts_unix: last.timestamp(),
            label: "backup_complete".into(),
        });
    }
    Ok(pts)
}
