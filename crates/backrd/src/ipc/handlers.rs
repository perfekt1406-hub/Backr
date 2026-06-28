/*
 * ipc/handlers.rs — IPC method dispatch for the backrd daemon.
 *
 * `dispatch` is the single entry point called by the connection handler after
 * deserialising each `IpcRequest`. It matches on the method name and calls the
 * appropriate handler, returning either a JSON result payload or an `IpcError`.
 *
 * Only `ping` and `resolve_shell_bootstrap` are implemented in U3; the full set
 * of 26 handlers will be wired in subsequent units (U4–U6).
 */

use std::sync::Arc;

use serde_json::Value;

use crate::daemon_state::DaemonState;
use crate::ipc::protocol::IpcError;

/// Dispatches an incoming IPC request to the appropriate stub handler.
///
/// # Parameters
/// - `method` — The method name string from `IpcRequest::method`.
/// - `params` — The raw JSON params object from `IpcRequest::params`.
/// - `state`  — Shared daemon state; individual handlers lock only the fields they need.
///
/// # Returns
/// `Ok(Value)` on success (to be serialised into `IpcResponse::result`), or
/// `Err(IpcError)` which the caller wraps into `IpcResponse::error`.
pub async fn dispatch(
    method: &str,
    params: Value,
    state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    match method {
        "ping" => handle_ping(params, state).await,
        "resolve_shell_bootstrap" => handle_resolve_shell_bootstrap(params, state).await,
        _ => Err(IpcError::new(
            "MethodNotFound",
            format!("unknown method: {method}"),
        )),
    }
}

// ---------------------------------------------------------------------------
// Individual handler stubs
// ---------------------------------------------------------------------------

/// Responds to a liveness probe with `{"pong": true}`.
///
/// Clients can use this to verify the daemon is running and the socket is
/// accepting connections before sending heavier requests.
async fn handle_ping(_params: Value, _state: Arc<DaemonState>) -> Result<Value, IpcError> {
    Ok(serde_json::json!({ "pong": true }))
}

/// Stub: returns a placeholder shell bootstrap mode.
///
/// The full implementation (U5) will inspect the host config and persisted
/// pairing state to decide between `"setup"`, `"paired"`, and `"error"`.
/// For now it always returns `{"mode": "setup"}` so callers have something
/// to pattern-match against while the rest of the daemon is wired up.
async fn handle_resolve_shell_bootstrap(
    _params: Value,
    _state: Arc<DaemonState>,
) -> Result<Value, IpcError> {
    Ok(serde_json::json!({ "mode": "setup" }))
}
