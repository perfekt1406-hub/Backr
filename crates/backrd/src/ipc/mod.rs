/*
 * ipc/mod.rs — Unix socket connection handler for the backrd IPC server.
 *
 * Each accepted `UnixStream` is handed to `handle_connection`, which runs an
 * NDJSON read-dispatch-write loop until the client disconnects or an
 * unrecoverable I/O error occurs. One Tokio task is spawned per connection so
 * clients are fully independent.
 *
 * Protocol summary (one JSON object per `\n`-terminated line):
 *   Client → Daemon : {"id": "<uuid>", "method": "<name>", "params": {…}}
 *   Daemon → Client : {"id": "<uuid>", "result": {…}}     (success)
 *                   | {"id": "<uuid>", "error": {…}}      (handler error)
 *                   | {"id": null,     "error": {…}}      (parse failure)
 */

pub mod handlers;
pub mod protocol;

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, error, warn};

use crate::daemon_state::DaemonState;
use crate::ipc::protocol::{IpcError, IpcRequest, IpcResponse};

/// Drives the NDJSON read-dispatch-write loop for a single client connection.
///
/// The function returns when the client sends EOF, closes the connection, or an
/// I/O error prevents further communication.
///
/// # Parameters
/// - `stream` — The accepted Unix domain socket stream.
/// - `state`  — Shared daemon state passed into each handler.
pub async fn handle_connection(stream: UnixStream, state: Arc<DaemonState>) {
    // Split the stream so we can read and write concurrently from the same socket.
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();

        // Read one newline-terminated JSON line; 0 bytes means EOF (clean close).
        let n = match reader.read_line(&mut line).await {
            Ok(n) => n,
            Err(e) => {
                warn!("ipc: read error: {e}");
                break;
            }
        };

        if n == 0 {
            debug!("ipc: client disconnected");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Deserialise the request; respond with a parse-error if the JSON is invalid.
        let request: IpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let response = malformed_request_response(e);
                if let Err(write_err) = write_response(&mut write_half, &response).await {
                    error!("ipc: failed to write parse-error response: {write_err}");
                    break;
                }
                continue;
            }
        };

        debug!("ipc: {} → method={}", request.id, request.method);

        // Dispatch to the appropriate handler stub.
        let response = match handlers::dispatch(&request.method, request.params, Arc::clone(&state)).await {
            Ok(result) => IpcResponse::ok(&request.id, result),
            Err(err) => IpcResponse::err(&request.id, err),
        };

        if let Err(e) = write_response(&mut write_half, &response).await {
            error!("ipc: failed to write response for {}: {e}", request.id);
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Serialises `response` to JSON, appends `\n`, and flushes it onto `writer`.
///
/// Returns an I/O error if serialisation or the write itself fails.
async fn write_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    response: &IpcResponse,
) -> std::io::Result<()> {
    // Serialisation failure is an internal bug; convert to an I/O error so the
    // caller can treat it uniformly.
    let mut json = serde_json::to_string(response).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::Other, format!("serialise error: {e}"))
    })?;
    json.push('\n');
    writer.write_all(json.as_bytes()).await?;
    writer.flush().await
}

/// Builds the error response emitted when a client sends malformed JSON.
///
/// Uses a literal `"null"` id string because we could not parse a real id from
/// the broken request; clients should treat `"null"` as the sentinel for a
/// parse-level failure.
fn malformed_request_response(parse_err: serde_json::Error) -> IpcResponse {
    IpcResponse::err(
        "null",
        IpcError::new(
            "InvalidInput",
            format!("failed to parse request: {parse_err}"),
        ),
    )
}
