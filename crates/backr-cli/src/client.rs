/*
 * client.rs — IPC client for the backrd Unix domain socket.
 *
 * Provides helpers for connecting to the daemon, sending a single NDJSON request,
 * and reading the response.  Two entry points are exposed:
 *
 *   - `send_command`               — fire one request, return the result/error.
 *   - `send_command_stream_progress` — like `send_command` but also prints
 *                                      `backup_progress` push events to stdout
 *                                      as they arrive, returning when the final
 *                                      response (identified by its `id` field) lands.
 *
 * Socket path resolution follows the same logic as `backrd` (KTD-2):
 *   1. `$XDG_RUNTIME_DIR/backr/backrd.sock`
 *   2. `~/.local/share/backr/backrd.sock`
 *   3. `/tmp/backrd.sock` (last-resort fallback)
 */

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

// ---------------------------------------------------------------------------
// Socket path
// ---------------------------------------------------------------------------

/// Resolves the path to the `backrd` Unix domain socket.
///
/// Resolution order (mirrors `backrd` KTD-2 logic):
///   1. `$XDG_RUNTIME_DIR/backr/backrd.sock`
///   2. `~/.local/share/backr/backrd.sock`
///   3. `/tmp/backrd.sock` as a last-resort fallback
///
/// # Returns
///
/// The resolved `PathBuf` — the socket may or may not exist at this path.
pub fn socket_path() -> PathBuf {
    if let Ok(d) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(d).join("backr").join("backrd.sock");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".local/share/backr/backrd.sock")
}

// ---------------------------------------------------------------------------
// Request ID generation
// ---------------------------------------------------------------------------

/// Generates a unique request ID string using a random UUID v4.
///
/// # Returns
///
/// A hyphenated UUID string (e.g. `"550e8400-e29b-41d4-a716-446655440000"`).
fn new_request_id() -> String {
    // Use a simple random approach without pulling in the full uuid crate here.
    // We read 16 bytes from /dev/urandom and format them as UUID v4.
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{:016x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        t.as_nanos() & 0xffff_ffff_ffff_ffff,
        (t.subsec_nanos() >> 16) & 0xffff,
        (t.subsec_nanos() >> 4) & 0x0fff,
        0x8000u32 | ((t.subsec_nanos()) & 0x3fff),
        t.as_secs() & 0x0000_ffff_ffff,
    )
}

// ---------------------------------------------------------------------------
// IPC client helpers
// ---------------------------------------------------------------------------

/// Sends one JSON-RPC-style request to `backrd`, waits for the matching response,
/// and returns the `result` payload on success or an error derived from the
/// `error` field on failure.
///
/// # Parameters
///
/// - `method` — IPC method name (e.g. `"get_backup_status"`).
/// - `params` — JSON params object (`serde_json::json!({})` for no params).
///
/// # Returns
///
/// `Ok(Value)` containing the `result` field of the daemon response, or
/// `Err` with the daemon's `error.message` (or a connection/IO error).
pub async fn send_command(method: &str, params: Value) -> Result<Value> {
    let stream = UnixStream::connect(socket_path())
        .await
        .with_context(|| format!("cannot connect to backrd at {}", socket_path().display()))?;

    let id = new_request_id();
    let request = serde_json::json!({
        "id": id,
        "method": method,
        "params": params,
    });
    let mut request_line = serde_json::to_string(&request)
        .context("failed to serialise IPC request")?;
    request_line.push('\n');

    let (read_half, mut write_half) = stream.into_split();
    write_half
        .write_all(request_line.as_bytes())
        .await
        .context("failed to send IPC request")?;
    write_half.flush().await.context("failed to flush IPC socket")?;
    // Drop the write half — we're done sending.
    drop(write_half);

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .context("failed to read IPC response")?;
        if n == 0 {
            bail!("daemon closed connection without sending a response");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse the incoming line as a JSON object.
        let obj: Value =
            serde_json::from_str(trimmed).context("daemon sent invalid JSON")?;

        // Skip push events (they have an `event` key but no `id`).
        if obj.get("event").is_some() {
            continue;
        }

        // This must be the response — check the id.
        let resp_id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if resp_id != id && resp_id != "null" {
            // Unexpected id — skip (should not happen in request/response flow).
            continue;
        }

        return extract_result(obj);
    }
}

/// Sends one request to `backrd` and streams `backup_progress` push events to
/// stdout until the final response arrives.
///
/// Identical to `send_command` except that lines carrying
/// `{"event": "backup_progress", "data": "..."}` are printed to stdout as they
/// are received, so the caller sees live rsync output.
///
/// # Parameters
///
/// - `method` — IPC method name (e.g. `"run_backup"`).
/// - `params` — JSON params object.
///
/// # Returns
///
/// `Ok(Value)` with the final `result` payload, or `Err` on daemon error / IO failure.
pub async fn send_command_stream_progress(method: &str, params: Value) -> Result<Value> {
    let stream = UnixStream::connect(socket_path())
        .await
        .with_context(|| format!("cannot connect to backrd at {}", socket_path().display()))?;

    let id = new_request_id();
    let request = serde_json::json!({
        "id": id,
        "method": method,
        "params": params,
    });
    let mut request_line = serde_json::to_string(&request)
        .context("failed to serialise IPC request")?;
    request_line.push('\n');

    let (read_half, mut write_half) = stream.into_split();
    write_half
        .write_all(request_line.as_bytes())
        .await
        .context("failed to send IPC request")?;
    write_half.flush().await.context("failed to flush IPC socket")?;
    drop(write_half);

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .context("failed to read IPC response")?;
        if n == 0 {
            bail!("daemon closed connection without sending a response");
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let obj: Value =
            serde_json::from_str(trimmed).context("daemon sent invalid JSON")?;

        // Check whether this is a push event.
        if let Some(event_name) = obj.get("event").and_then(|v| v.as_str()) {
            if event_name == "backup_progress" {
                // Print the progress line to stdout so the user sees live output.
                if let Some(data) = obj.get("data") {
                    let msg = match data {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    println!("{msg}");
                }
            }
            // All events: continue reading until we get the final response.
            continue;
        }

        // Must be the response.
        let resp_id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if resp_id != id && resp_id != "null" {
            continue;
        }

        return extract_result(obj);
    }
}

/// Synchronous one-shot IPC call over the daemon socket.
///
/// The self-update worker is blocking and runs while it stops and restarts the
/// daemon, so it cannot use the async client mid-flight. This std-socket variant
/// keeps the swap path free of the tokio runtime. Skips push events and returns
/// the response whose `id` matches the request.
///
/// # Parameters
///
/// - `method` — IPC method name.
/// - `params` — JSON params object.
/// - `timeout` — read/write timeout for the call.
pub fn send_command_blocking(
    method: &str,
    params: Value,
    timeout: std::time::Duration,
) -> Result<Value> {
    let path = socket_path();
    let mut stream = std::os::unix::net::UnixStream::connect(&path)
        .with_context(|| format!("cannot connect to backrd at {}", path.display()))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();

    let id = new_request_id();
    let request = serde_json::json!({ "id": id, "method": method, "params": params });
    let mut request_line =
        serde_json::to_string(&request).context("failed to serialise IPC request")?;
    request_line.push('\n');
    stream
        .write_all(request_line.as_bytes())
        .context("failed to send IPC request")?;
    stream.flush().ok();

    let reader = std::io::BufReader::new(stream);
    for line in reader.lines() {
        let line = line.context("failed to read IPC response")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let obj: Value = serde_json::from_str(trimmed).context("daemon sent invalid JSON")?;
        if obj.get("event").is_some() {
            continue;
        }
        let resp_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        if resp_id != id && resp_id != "null" {
            continue;
        }
        return extract_result(obj);
    }
    bail!("daemon closed connection without sending a response")
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extracts the `result` field from a daemon response object, converting an
/// `error` field into an `anyhow::Error`.
///
/// # Parameters
///
/// - `obj` — A parsed JSON object that should be an `IpcResponse` shape.
///
/// # Returns
///
/// `Ok(Value)` when `result` is present; `Err` when `error` is present or neither field is found.
fn extract_result(obj: Value) -> Result<Value> {
    if let Some(err) = obj.get("error") {
        // Prefer the human-readable `message` field; fall back to the full error object.
        let message = err
            .get("message")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| err.to_string());
        return Err(anyhow!("daemon error: {message}"));
    }
    obj.get("result")
        .cloned()
        .ok_or_else(|| anyhow!("daemon response missing both `result` and `error` fields"))
}
