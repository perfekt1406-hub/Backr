/*
 * ipc/mod.rs — Unix socket connection handler for the backrd IPC server.
 *
 * Each accepted `UnixStream` is handed to `handle_connection`, which runs three
 * concurrent sub-tasks per connection:
 *
 *   1. **Read-dispatch loop** — reads NDJSON requests from the client, dispatches
 *      them to `handlers::dispatch`, and queues the serialised response on the
 *      shared write channel.
 *   2. **Event-forward task** — receives `IpcEvent` messages from a broadcast
 *      channel (produced by `IpcBroadcastSink`) and queues them on the same
 *      shared write channel.
 *   3. **Writer task** — drains an unbounded MPSC channel of JSON-serialised lines
 *      and writes them to the socket in order, appending `\n` to each.
 *
 * Using a single MPSC write channel means the socket write half never needs to be
 * shared or locked — only the writer task touches it.
 *
 * The `event_tx` broadcast sender is threaded through to `handlers::dispatch` so that
 * handlers like `run_backup` and `restore_*` can construct an `IpcBroadcastSink` to
 * stream progress events to all connected GUI clients.
 *
 * Protocol summary (one JSON object per `\n`-terminated line):
 *   Client → Daemon : {"id": "<uuid>", "method": "<name>", "params": {…}}
 *   Daemon → Client : {"id": "<uuid>", "result": {…}}      (success)
 *                   | {"id": "<uuid>", "error": {…}}       (handler error)
 *                   | {"event": "<name>", "data": {…}}     (push event)
 *                   | {"id": "null",     "error": {…}}     (parse failure)
 */

pub mod handlers;
pub mod protocol;

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, warn};

use crate::daemon_state::DaemonState;
use crate::ipc::protocol::{IpcError, IpcEvent, IpcRequest, IpcResponse};

/// Drives the NDJSON read-dispatch-write loop for a single client connection.
///
/// Spawns an event-forwarder task and a socket-writer task alongside the
/// read-dispatch loop so all three can proceed concurrently.  Returns when the
/// client sends EOF, closes the connection, or a write error terminates the
/// writer task; all helper tasks are aborted on exit.
///
/// # Parameters
/// - `stream`   — The accepted Unix domain socket stream.
/// - `state`    — Shared daemon state passed into each handler.
/// - `event_tx` — Broadcast sender; handlers that produce progress events (backup,
///                restore) clone this to construct an `IpcBroadcastSink`.
/// - `event_rx` — Subscription to the daemon-wide `IpcEvent` broadcast channel;
///                events received here are forwarded to this connection's socket.
pub async fn handle_connection(
    stream: UnixStream,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
    event_rx: broadcast::Receiver<IpcEvent>,
) {
    // Split the stream so the reader and writer can operate concurrently.
    let (read_half, write_half) = stream.into_split();

    // Single-writer channel: both the request dispatcher and the event forwarder
    // send serialised JSON lines here; the writer task drains this channel.
    let (write_tx, write_rx) = mpsc::unbounded_channel::<String>();

    // Spawn the socket writer task.
    let writer_handle = tokio::spawn(run_writer(write_half, write_rx));

    // Spawn the event forwarder task (broadcast → write channel).
    let fwd_tx = write_tx.clone();
    let forwarder_handle = tokio::spawn(run_event_forwarder(event_rx, fwd_tx));

    // Run the request read-dispatch loop on the current task.
    run_request_loop(read_half, state, event_tx, write_tx).await;

    // When the reader loop exits (client EOF or error), abort the helpers.
    forwarder_handle.abort();
    writer_handle.abort();

    // Drain both task results (they were aborted so they finish immediately).
    let _ = tokio::join!(forwarder_handle, writer_handle);
}

// ---------------------------------------------------------------------------
// Inner task functions
// ---------------------------------------------------------------------------

/// Reads NDJSON requests from the socket, dispatches each to a handler, and
/// queues the serialised response on `write_tx`.
///
/// Returns when the client sends EOF or a read error occurs.
///
/// # Parameters
/// - `read_half` — Owned read half of the Unix socket.
/// - `state`     — Shared daemon state.
/// - `event_tx`  — Broadcast sender passed to handlers that emit progress events.
/// - `write_tx`  — MPSC sender to the socket writer task.
async fn run_request_loop(
    read_half: tokio::net::unix::OwnedReadHalf,
    state: Arc<DaemonState>,
    event_tx: broadcast::Sender<IpcEvent>,
    write_tx: mpsc::UnboundedSender<String>,
) {
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
                if send_json(&write_tx, &response).is_err() {
                    // Writer task is gone — no point continuing.
                    break;
                }
                continue;
            }
        };

        debug!("ipc: {} → method={}", request.id, request.method);

        // Dispatch to the appropriate handler, passing event_tx for progress sinks.
        let response = match handlers::dispatch(
            &request.method,
            request.params,
            Arc::clone(&state),
            event_tx.clone(),
        )
        .await
        {
            Ok(result) => IpcResponse::ok(&request.id, result),
            Err(err) => IpcResponse::err(&request.id, err),
        };

        if send_json(&write_tx, &response).is_err() {
            error!("ipc: writer task closed for {}", request.id);
            break;
        }
    }
}

/// Forwards `IpcEvent` messages from the broadcast channel to the connection's
/// write channel until the broadcast sender is dropped or the write channel closes.
///
/// Lagged messages (receiver too slow) are logged and skipped; the broadcast
/// receiver handles the lag internally without needing a manual re-subscribe.
///
/// # Parameters
/// - `event_rx` — Broadcast receiver subscription for this connection.
/// - `write_tx` — MPSC sender to the per-connection socket writer task.
async fn run_event_forwarder(
    mut event_rx: broadcast::Receiver<IpcEvent>,
    write_tx: mpsc::UnboundedSender<String>,
) {
    loop {
        match event_rx.recv().await {
            Ok(event) => {
                if send_json(&write_tx, &event).is_err() {
                    // Writer is gone — connection is closing.
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                // The connection handler fell behind; skip the missed events and continue.
                warn!("ipc: event forwarder skipped {n} lagged events");
            }
            Err(broadcast::error::RecvError::Closed) => {
                // Broadcast sender dropped (daemon shutting down).
                break;
            }
        }
    }
}

/// Drains the MPSC write channel and writes each line to the socket, appending `\n`.
///
/// Returns when the channel is closed (all senders dropped) or a write error occurs.
///
/// # Parameters
/// - `write_half` — Owned write half of the Unix socket.
/// - `write_rx`   — MPSC receiver; each message is a complete JSON line (no trailing `\n`).
async fn run_writer(
    mut write_half: tokio::net::unix::OwnedWriteHalf,
    mut write_rx: mpsc::UnboundedReceiver<String>,
) {
    while let Some(mut json) = write_rx.recv().await {
        json.push('\n');
        if let Err(e) = write_half.write_all(json.as_bytes()).await {
            error!("ipc: socket write error: {e}");
            break;
        }
        if let Err(e) = write_half.flush().await {
            error!("ipc: socket flush error: {e}");
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Serialises `value` to a JSON string and sends it into `write_tx`.
///
/// Returns `Err(())` if serialisation fails (internal bug) or if the channel is
/// closed (writer task gone).
///
/// # Parameters
/// - `write_tx` — MPSC sender to the socket writer task.
/// - `value`    — Any `serde::Serialize` value to send as a JSON line.
fn send_json<T: serde::Serialize>(
    write_tx: &mpsc::UnboundedSender<String>,
    value: &T,
) -> Result<(), ()> {
    let json = serde_json::to_string(value).map_err(|e| {
        error!("ipc: serialise error: {e}");
    })?;
    write_tx.send(json).map_err(|_| ())
}

/// Builds the error response emitted when a client sends malformed JSON.
///
/// Uses a literal `"null"` id string because we could not parse a real id from
/// the broken request; clients should treat `"null"` as the sentinel for a
/// parse-level failure.
///
/// # Parameters
/// - `parse_err` — The `serde_json` error describing why the request was invalid.
fn malformed_request_response(parse_err: serde_json::Error) -> IpcResponse {
    IpcResponse::err(
        "null",
        IpcError::new(
            "InvalidInput",
            format!("failed to parse request: {parse_err}"),
        ),
    )
}
