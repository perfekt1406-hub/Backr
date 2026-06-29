/*
 * Host-side and client-side pairing commands proxied to the backrd daemon.
 *
 * The daemon owns all pairing state (mDNS advertisement, listener socket, pairing sessions).
 * Each Tauri command is a thin IPC proxy that forwards the request and returns the response.
 *
 * The function signatures are kept identical to preserve the frontend `invoke()` call contract.
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;

use tauri::State;

use crate::config::Config;
use crate::error::BackrCommandError;
use crate::pairing::client::PairDraft;
use crate::pairing::discovery::DiscoveredHost;
use crate::state::AppState;

/// Returned to the host UI when a pairing window opens.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PairingStarted {
    /// 6-digit code to show on the host.
    pub code: String,
}

/// Opens a pairing window on the daemon (code + mDNS advertise + listener).
///
/// # Inputs
///
/// * `state` — managed [`AppState`] (unused by proxy; kept for signature compatibility).
///
/// # Returns
///
/// [`PairingStarted`] with the 6-digit code to display on the host screen.
#[tauri::command]
pub async fn start_pairing(
    _state: State<'_, Arc<AppState>>,
) -> Result<PairingStarted, BackrCommandError> {
    let v = crate::ipc_client::send("start_pairing", serde_json::json!({})).await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::pairing(format!("failed to deserialize pairing start: {e}")))
}

/// Closes the pairing window on the daemon if one is open.
///
/// # Inputs
///
/// * `state` — managed [`AppState`] (unused by proxy; kept for signature compatibility).
#[tauri::command]
pub async fn stop_pairing(_state: State<'_, Arc<AppState>>) -> Result<(), BackrCommandError> {
    crate::ipc_client::send("stop_pairing", serde_json::json!({})).await?;
    Ok(())
}

/// Returns true while the daemon has a pairing window open.
///
/// # Inputs
///
/// * `state` — managed [`AppState`] (unused by proxy; kept for signature compatibility).
#[tauri::command]
pub async fn pairing_status(
    _state: State<'_, Arc<AppState>>,
) -> Result<bool, BackrCommandError> {
    let v = crate::ipc_client::send("pairing_status", serde_json::json!({})).await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::pairing(format!("failed to deserialize pairing status: {e}")))
}

/// Browses the LAN for hosts currently in pairing mode (~2.5 s window) via the daemon.
///
/// # Returns
///
/// A list of discovered hosts ready for pairing.
#[tauri::command]
pub async fn discover_hosts() -> Result<Vec<DiscoveredHost>, BackrCommandError> {
    let v = crate::ipc_client::send("discover_hosts", serde_json::json!({})).await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::pairing(format!("failed to deserialize discovered hosts: {e}")))
}

/// Pairs this laptop with a discovered host using the 6-digit code.
///
/// Returns a `PairDraft` containing the prefilled config AND the host's SSH key fingerprint.
/// The caller must show the fingerprint to the user for out-of-band verification before calling
/// `confirm_pairing` to finalize.
///
/// # Inputs
///
/// * `address` — "ip:port" from `discover_hosts`.
/// * `code`    — 6-digit code shown on the host screen.
#[tauri::command]
pub async fn pair_with_host(address: String, code: String) -> Result<PairDraft, BackrCommandError> {
    let v = crate::ipc_client::send(
        "pair_with_host",
        serde_json::json!({ "address": address, "code": code }),
    )
    .await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::pairing(format!("failed to deserialize pair draft: {e}")))
}

/// Finalizes a confirmed pair: pins the host's SSH key and returns the ready-to-save config.
///
/// Call this only after the user has verified the fingerprint shown in the UI matches what is
/// displayed on the host's Backr screen.
///
/// # Inputs
///
/// * `draft` — the `PairDraft` returned by `pair_with_host`.
///
/// # Returns
///
/// The finalized `Config` on success.
#[tauri::command]
pub async fn confirm_pairing(draft: PairDraft) -> Result<Config, BackrCommandError> {
    let v = crate::ipc_client::send(
        "confirm_pairing",
        serde_json::json!({ "draft": draft }),
    )
    .await?;
    serde_json::from_value(v)
        .map_err(|e| BackrCommandError::pairing(format!("failed to deserialize confirmed config: {e}")))
}

