/*
 * Thin async client for the backrd Unix domain socket IPC protocol.
 *
 * All Tauri commands are thin proxies that call `send` or `send_with_progress`
 * here.  The daemon owns all business logic; this module only handles framing
 * (NDJSON), event routing (backup_progress side-channel), and error mapping.
 *
 * The wire types (`IpcRequest`/`IpcResponse`/`IpcEvent`/`IpcError`) are the
 * shared definitions in `backr_core::ipc_protocol`, identical to the ones the
 * daemon uses — so a request this client builds is exactly what the daemon
 * deserializes (the `id` field is a `String` on both ends, enforced at compile
 * time).
 *
 * Protocol:
 *   → one NDJSON line: `{ "id": "1", "method": "…", "params": { … } }\n`
 *   ← zero or more event lines: `{ "event": "backup_progress", "data": "…" }\n`
 *   ← one final response line:  `{ "id": "1", "result": … }\n`
 *                            or `{ "id": "1", "error": { "kind": "…", "message": "…" } }\n`
 */

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use backr_core::ipc_protocol::{IpcError, IpcEvent, IpcRequest, IpcResponse};

use crate::backup::rsync::BACKUP_PROGRESS_EVENT;
use crate::error::{BackrCommandError, ErrorKind};

/// Fixed request id used for every call (single-request-per-connection model).
///
/// The daemon's `IpcRequest.id` is a `String`, so this MUST serialize as a JSON
/// string. Sending a JSON number makes the daemon reject the request as a parse
/// error (`invalid type: integer, expected a string`) while holding the connection
/// open, which hangs the client's read loop forever.
const REQUEST_ID: &str = "1";

/// Idle (inter-line) read timeout for plain request/response calls.
///
/// These commands (config, bootstrap, status, …) reply effectively instantly, so
/// a connected-but-silent daemon — e.g. one that accepted the connection but
/// never answers — is a fault. Bounding the read turns that into a fast, legible
/// error (surfaced by the GUI's "can't reach backrd" screen) instead of an
/// indefinite hang. The timeout resets on every line received.
const PLAIN_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(20);

/// Idle (inter-line) read timeout for progress-streaming calls (backup/restore).
///
/// rsync streams `--info=progress2` lines continuously, so any real transfer
/// keeps resetting this. The bound only trips on total silence, generously sized
/// so a slow initial file-list scan never false-trips while a genuinely wedged
/// connection is still eventually surfaced rather than hanging forever.
const STREAM_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

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

/// Converts a daemon [`IpcError`] into a [`BackrCommandError`] by mapping the
/// kind string to the closest [`ErrorKind`] variant.
///
/// Unknown kind strings fall through to `InvalidInput` so callers still receive a
/// useful message.
///
/// # Inputs
///
/// * `err` — the `error` field of a daemon [`IpcResponse`].
///
/// # Returns
///
/// A [`BackrCommandError`] with a mapped kind and the daemon's message.
fn map_daemon_error(err: &IpcError) -> BackrCommandError {
    let kind = match err.kind.as_str() {
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

    BackrCommandError {
        kind,
        message: err.message.clone(),
    }
}

/// Returns true when `parsed` is the final response line for our request — i.e. it
/// carries an `id` matching `request_id`.
///
/// The daemon types the response `id` as a `String` and echoes it verbatim, so the
/// match is done on the string value (not `as_u64`, which never matches a string id
/// and would silently leave the read loop blocking forever). Event lines (which have
/// no `id` key) and unrelated lines return false.
///
/// # Inputs
///
/// * `parsed`     — one parsed NDJSON line received from the daemon.
/// * `request_id` — the id string this client sent.
///
/// # Returns
///
/// `true` if the line is the matching final response; `false` otherwise.
fn is_final_response(parsed: &Value, request_id: &str) -> bool {
    parsed.get("id").and_then(Value::as_str) == Some(request_id)
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

    // Build and send the NDJSON request line. Constructing the shared `IpcRequest`
    // (rather than an ad-hoc JSON object) guarantees the wire shape the daemon
    // expects — in particular a string `id`, which a numeric literal silently broke.
    let request = IpcRequest {
        id: REQUEST_ID.to_string(),
        method: method.to_string(),
        params,
    };
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

    // Streaming calls (backup/restore) get a generous idle bound; plain calls a
    // tight one. Either way a connected-but-silent daemon can't hang us forever.
    let idle_timeout = if app_opt.is_some() {
        STREAM_READ_IDLE_TIMEOUT
    } else {
        PLAIN_READ_IDLE_TIMEOUT
    };

    loop {
        // Bound each read so a daemon that accepted the connection but never
        // answers surfaces as a clear error instead of blocking indefinitely.
        let raw = match tokio::time::timeout(idle_timeout, lines.next_line()).await {
            Err(_elapsed) => {
                return Err(BackrCommandError::io(format!(
                    "backrd accepted the connection but sent no response within {}s",
                    idle_timeout.as_secs()
                )));
            }
            Ok(read) => read
                .map_err(|e| {
                    BackrCommandError::io(format!("error reading from backrd socket: {e}"))
                })?
                .ok_or_else(|| {
                    BackrCommandError::io(
                        "backrd socket closed before sending a response".to_string(),
                    )
                })?,
        };

        let parsed: Value = serde_json::from_str(&raw).map_err(|e| {
            BackrCommandError::io(format!("invalid JSON from backrd: {e} (raw: {raw:?})"))
        })?;

        // Event lines (identified by the `event` key, no `id`): re-emit progress
        // and keep reading.
        if parsed.get("event").is_some() {
            if let Ok(ev) = serde_json::from_value::<IpcEvent>(parsed) {
                if ev.event == "backup_progress" {
                    if let Some(app) = app_opt {
                        // Re-emit as the same event the frontend already subscribes to.
                        let _ = app.emit(BACKUP_PROGRESS_EVENT, ev.data);
                    }
                }
                // Other event types are silently ignored for forward-compatibility.
            }
            continue;
        }

        // Final response for our request id (the daemon echoes the String id verbatim).
        if is_final_response(&parsed, REQUEST_ID) {
            let response: IpcResponse = serde_json::from_value(parsed).map_err(|e| {
                BackrCommandError::io(format!("malformed response from backrd: {e}"))
            })?;
            if let Some(err) = response.error {
                return Err(map_daemon_error(&err));
            }
            // Success: return the result value (may be null for void commands).
            return Ok(response.result.unwrap_or(Value::Null));
        }

        // Line has neither a matching id nor an event — ignore and keep reading.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shared `IpcRequest` types `id` as a `String`, so the client cannot emit a
    /// numeric id — the drift that previously hung every call. Guard both that our id
    /// serializes as a JSON string and that a numeric id is rejected on deserialize
    /// (the daemon's exact failure mode).
    #[test]
    fn request_id_is_a_json_string() {
        let request = IpcRequest {
            id: REQUEST_ID.to_string(),
            method: "get_config".to_string(),
            params: json!({}),
        };
        let wire = serde_json::to_value(&request).expect("serialize request");
        assert!(
            wire["id"].is_string(),
            "id must serialize as a JSON string to satisfy the daemon's String-typed id"
        );
        assert_eq!(wire["id"], json!("1"));

        // A request with a numeric id (the old hand-rolled client) fails to
        // deserialize into the shared type — exactly how the daemon rejected it.
        let numeric = json!({ "id": 1, "method": "get_config", "params": {} });
        assert!(serde_json::from_value::<IpcRequest>(numeric).is_err());
    }

    /// A daemon error response deserializes into the shared `IpcResponse`/`IpcError`
    /// (with `result` absent → `None`) and maps to a `BackrCommandError` carrying the
    /// daemon's kind and message.
    #[test]
    fn error_response_maps_to_command_error() {
        let line = json!({
            "id": "1",
            "error": { "kind": "NotConfigured", "message": "no config" },
        });
        let resp: IpcResponse = serde_json::from_value(line).expect("deserialize response");
        let err = resp.error.expect("error field present");
        let mapped = map_daemon_error(&err);
        assert_eq!(mapped.message, "no config");
        assert!(matches!(mapped.kind, ErrorKind::NotConfigured));
    }

    /// The daemon echoes the request id back as a string; the matcher must compare
    /// it as a string for the read loop to recognize the final response.
    #[test]
    fn final_response_matches_on_string_id() {
        assert!(is_final_response(
            &json!({ "id": "1", "result": null }),
            REQUEST_ID
        ));
    }

    /// Regression guards for the original contract bug: a numeric id (old client),
    /// the daemon's `"null"` parse-error reply, and a different request id must all
    /// be treated as "not our response" rather than silently matched or mis-handled.
    #[test]
    fn final_response_rejects_numeric_or_mismatched_id() {
        assert!(!is_final_response(
            &json!({ "id": 1, "result": null }),
            REQUEST_ID
        ));
        assert!(!is_final_response(
            &json!({ "id": "null", "error": {} }),
            REQUEST_ID
        ));
        assert!(!is_final_response(
            &json!({ "id": "2", "result": null }),
            REQUEST_ID
        ));
    }
}
