/*
 * Thin async client for the backrd Unix domain socket IPC protocol.
 *
 * All Tauri commands are thin proxies that call `send` or `send_with_progress`
 * here.  The daemon owns all business logic; this module only handles framing
 * (NDJSON), event routing (backup_progress side-channel), and error mapping.
 *
 * Protocol:
 *   → one NDJSON line: `{ "id": 1, "method": "…", "params": { … } }\n`
 *   ← zero or more event lines: `{ "event": "backup_progress", "data": "…" }\n`
 *   ← one final response line:  `{ "id": 1, "result": … }\n`
 *                            or `{ "id": 1, "error": { "kind": "…", "message": "…" } }\n`
 */

use std::path::PathBuf;

use serde_json::Value;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::backup::rsync::BACKUP_PROGRESS_EVENT;
use crate::error::{BackrCommandError, ErrorKind};

/// Fixed JSON-RPC-like request id used for every call (single-request-per-connection model).
const REQUEST_ID: u64 = 1;

/// Resolves the canonical path of the backrd Unix domain socket.
///
/// Follows XDG_RUNTIME_DIR when set, mirroring the daemon's own socket path logic.
/// Falls back to `~/.local/share/backr/backrd.sock` on systems without a runtime dir.
///
/// # Returns
///
/// Absolute `PathBuf` to the socket file (may not exist yet if the daemon is stopped).
pub fn socket_path() -> PathBuf {
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(d).join("backr").join("backrd.sock")
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".local/share/backr/backrd.sock")
    }
}

/// Converts a daemon error JSON object `{ "kind": "…", "message": "…" }` into a
/// [`BackrCommandError`] by mapping the kind string to the closest [`ErrorKind`] variant.
///
/// Unknown kind strings fall through to `InvalidInput` so callers still receive a useful message.
///
/// # Inputs
///
/// * `err_obj` — parsed JSON value (object) from the `"error"` field of a daemon response.
///
/// # Returns
///
/// A [`BackrCommandError`] with a mapped kind and the daemon's message.
fn map_daemon_error(err_obj: &Value) -> BackrCommandError {
    let message = err_obj
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown daemon error")
        .to_string();

    let kind = match err_obj
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
    {
        "NotConfigured" => ErrorKind::NotConfigured,
        "SshFailed" => ErrorKind::SshFailed,
        "RsyncFailed" => ErrorKind::RsyncFailed,
        "Io" => ErrorKind::Io,
        "BackupInProgress" => ErrorKind::BackupInProgress,
        "InvalidInput" => ErrorKind::InvalidInput,
        "Config" => ErrorKind::Config,
        "Pairing" => ErrorKind::Pairing,
        "TaskFailed" => ErrorKind::TaskFailed,
        _ => ErrorKind::InvalidInput,
    };

    BackrCommandError { kind, message }
}

/// Connects to the backrd socket, sends one JSON-RPC request, and returns the result value.
///
/// Reads lines until a line containing the matching `"id"` is found; any `"event"` lines
/// encountered before the response are silently discarded (use [`send_with_progress`] to
/// forward them).
///
/// # Inputs
///
/// * `method` — daemon method name (e.g. `"get_config"`).
/// * `params` — JSON object of method parameters (may be `json!({})`).
///
/// # Returns
///
/// `Ok(Value)` with the `"result"` field contents; `Err(BackrCommandError)` on any failure.
pub async fn send(method: &str, params: Value) -> Result<Value, BackrCommandError> {
    send_inner(method, params, None).await
}

/// Like [`send`], but also listens for `backup_progress` event lines and re-emits them via
/// the Tauri app handle before returning the final result.
///
/// Used by `run_backup`, `restore_snapshot`, `restore_all_snapshots`, and `restore_all_projects`
/// which stream incremental rsync output to the frontend while the daemon works.
///
/// # Inputs
///
/// * `method` — daemon method name.
/// * `params` — JSON object of method parameters.
/// * `app`    — live Tauri app handle used to emit `backup://progress` events to the webview.
///
/// # Returns
///
/// `Ok(Value)` with the final `"result"`; `Err(BackrCommandError)` on failure.
pub async fn send_with_progress(
    method: &str,
    params: Value,
    app: &tauri::AppHandle,
) -> Result<Value, BackrCommandError> {
    send_inner(method, params, Some(app)).await
}

/// Internal implementation shared by [`send`] and [`send_with_progress`].
///
/// Opens a fresh `UnixStream` connection per call (the daemon is assumed stateless per request).
/// Writes one NDJSON line, then reads lines until the matching final response arrives.
///
/// # Inputs
///
/// * `method`  — daemon method name.
/// * `params`  — JSON object of method parameters.
/// * `app_opt` — optional app handle for re-emitting `backup_progress` events.
///
/// # Returns
///
/// `Ok(Value)` from the `"result"` field; `Err` on I/O, daemon error, or protocol violation.
async fn send_inner(
    method: &str,
    params: Value,
    app_opt: Option<&tauri::AppHandle>,
) -> Result<Value, BackrCommandError> {
    // Connect to the daemon socket.
    let stream = UnixStream::connect(socket_path()).await.map_err(|e| {
        BackrCommandError::io(format!(
            "cannot connect to backrd socket ({}): {e}",
            socket_path().display()
        ))
    })?;

    let (read_half, mut write_half) = stream.into_split();

    // Build and send the NDJSON request line.
    let request = serde_json::json!({
        "id": REQUEST_ID,
        "method": method,
        "params": params,
    });
    let mut line = serde_json::to_string(&request).map_err(|e| {
        BackrCommandError::config(format!("failed to serialize IPC request: {e}"))
    })?;
    line.push('\n');

    write_half
        .write_all(line.as_bytes())
        .await
        .map_err(|e| BackrCommandError::io(format!("failed to write to backrd socket: {e}")))?;

    // Read response lines.  Side-channel event lines arrive before the final response.
    let reader = BufReader::new(read_half);
    let mut lines = reader.lines();

    loop {
        let raw = lines
            .next_line()
            .await
            .map_err(|e| BackrCommandError::io(format!("error reading from backrd socket: {e}")))?
            .ok_or_else(|| {
                BackrCommandError::io("backrd socket closed before sending a response".to_string())
            })?;

        let parsed: Value = serde_json::from_str(&raw).map_err(|e| {
            BackrCommandError::io(format!("invalid JSON from backrd: {e} (raw: {raw:?})"))
        })?;

        // If the line is an event, re-emit it and keep reading.
        if let Some(event_name) = parsed.get("event").and_then(Value::as_str) {
            if event_name == "backup_progress" {
                if let Some(app) = app_opt {
                    let data = parsed.get("data").cloned().unwrap_or(Value::Null);
                    // Re-emit as the same event the frontend already subscribes to.
                    let _ = app.emit(BACKUP_PROGRESS_EVENT, data);
                }
            }
            // All other event types are silently ignored for forward-compatibility.
            continue;
        }

        // Check if this is the final response for our request.
        if parsed.get("id").and_then(Value::as_u64) == Some(REQUEST_ID) {
            // Daemon returned an error object.
            if let Some(err_val) = parsed.get("error") {
                return Err(map_daemon_error(err_val));
            }

            // Success: return the result value (may be null for void commands).
            let result = parsed
                .get("result")
                .cloned()
                .unwrap_or(Value::Null);
            return Ok(result);
        }

        // Line has neither a matching id nor an event — ignore and keep reading.
    }
}
