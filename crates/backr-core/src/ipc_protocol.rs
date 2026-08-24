/*
 * ipc_protocol.rs — shared NDJSON IPC wire types for the backrd Unix socket.
 *
 * This is the single source of truth for the daemon⇄GUI protocol. Both the
 * daemon (`backrd`, which deserializes requests and serializes responses) and
 * the GUI client (`backr`'s `ipc_client`, which serializes requests and
 * deserializes responses) depend on these exact types, so the wire contract
 * cannot drift between the two ends without a compile error. (It previously
 * drifted: the client hand-built requests with a numeric `id` while the daemon
 * required a string, which silently hung every call.)
 *
 * Every message is a single JSON object terminated by `\n` (newline-delimited
 * JSON). Three message shapes are defined:
 *   - `IpcRequest`  — client → daemon: carries a unique `id`, a `method` name,
 *                     and a `params` payload.
 *   - `IpcResponse` — daemon → client: echoes the request `id` and contains
 *                     either a `result` or an `error`, never both.
 *   - `IpcEvent`    — daemon → client push notification: no `id`, carries an
 *                     `event` name and arbitrary `data`.
 *
 * All types derive both `Serialize` and `Deserialize` so either end can produce
 * or consume any shape.
 */

use serde::{Deserialize, Serialize};

/// A request sent by the GUI (or any IPC client) to the daemon.
///
/// Fields:
/// - `id`     — Caller-generated string id; echoed verbatim in the response so the
///   caller can correlate replies. MUST be a JSON string on the wire.
/// - `method` — Handler name (e.g. `"ping"`, `"resolve_shell_bootstrap"`).
/// - `params` — Arbitrary JSON object; may be `{}` when no parameters are needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// A response sent by the daemon back to a client in reply to an `IpcRequest`.
///
/// Exactly one of `result` or `error` is present in a well-formed response.
/// `serde` skips the `None` variant when serializing so the JSON never carries a
/// `null` key, and `#[serde(default)]` lets the absent variant deserialize to
/// `None` on the receiving end.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

impl IpcResponse {
    /// Build a success response carrying `result` as its payload.
    pub fn ok(id: impl Into<String>, result: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    pub fn err(id: impl Into<String>, error: IpcError) -> Self {
        Self {
            id: id.into(),
            result: None,
            error: Some(error),
        }
    }
}

/// Structured error embedded in `IpcResponse::error`.
///
/// - `kind`    — Machine-readable error category (e.g. `"MethodNotFound"`, `"InvalidInput"`).
/// - `message` — Human-readable explanation for logging and developer tooling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub kind: String,
    pub message: String,
}

impl IpcError {
    /// Convenience constructor.
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

/// An unsolicited push event emitted by the daemon to all connected clients.
///
/// No `id` field — clients distinguish events from responses by the presence of
/// the `event` key rather than an `id` key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    pub event: String,
    pub data: serde_json::Value,
}
