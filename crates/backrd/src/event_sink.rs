/*
 * event_sink.rs — IPC broadcast progress sink for backrd.
 *
 * Implements the `ProgressSink` trait from `backr_core::progress_sink` using a
 * `tokio::sync::broadcast::Sender<IpcEvent>`. Each call to `backup_progress_line`
 * broadcasts an `IpcEvent` with `event = "backup_progress"` and the progress line
 * as its JSON string data payload to every currently-subscribed connection handler.
 *
 * Send errors (no active receivers) are intentionally ignored: if no GUI is
 * connected there is nowhere to deliver progress, which is not an error condition.
 */

use tokio::sync::broadcast;

use backr_core::progress_sink::ProgressSink;

use crate::ipc::protocol::IpcEvent;

/// Broadcasts backup progress lines to all open IPC connections via a Tokio
/// broadcast channel.
///
/// Clone the underlying `broadcast::Sender` to share it across tasks; each
/// `IpcBroadcastSink` holds a sender handle independently.
pub struct IpcBroadcastSink {
    /// Sender half of the broadcast channel shared with all connection handlers.
    sender: broadcast::Sender<IpcEvent>,
}

impl IpcBroadcastSink {
    /// Constructs a new sink wrapping the provided broadcast sender.
    ///
    /// # Parameters
    /// - `sender` — Shared broadcast sender; typically cloned from the channel
    ///              created in `main` before the accept loop starts.
    pub fn new(sender: broadcast::Sender<IpcEvent>) -> Self {
        Self { sender }
    }
}

impl ProgressSink for IpcBroadcastSink {
    /// Broadcasts one progress line to all connected GUI clients.
    ///
    /// Serialises `line` as a JSON string in the `data` field of an `IpcEvent`
    /// with `event = "backup_progress"`. If no receivers are subscribed (i.e. no
    /// GUI is connected) the send error is silently discarded.
    ///
    /// # Parameters
    /// - `line` — One line of rsync / backup stdout or status output.
    fn backup_progress_line(&self, line: String) {
        // Ignore send errors — no receivers means no GUI is connected, which is fine.
        let _ = self.sender.send(IpcEvent {
            event: "backup_progress".into(),
            data: serde_json::json!(line),
        });
    }
}
