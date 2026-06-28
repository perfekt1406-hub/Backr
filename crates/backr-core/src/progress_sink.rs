/*
 * Abstraction for streaming rsync / backup status lines to either the Tauri webview or test collectors.
 * Keeps `backup::rsync` usable from integration tests and the daemon without a running `AppHandle`.
 *
 * This file contains the portable parts:
 *   - `ProgressSink` trait — the progress event consumer abstraction.
 *   - `SharedProgress` type alias — cloneable `Arc<dyn ProgressSink>` for async workers.
 *   - `CollectLines` — test/integration helper that accumulates lines in memory.
 *
 * The Tauri-specific `AppEmitProgress` implementation (which depends on `tauri::AppHandle`)
 * lives in `src-tauri/src/progress_sink.rs` and imports this trait.
 */

use std::sync::{Arc, Mutex};

/// Receives human-readable rsync progress or status lines (mirrors `backup://progress` payloads).
pub trait ProgressSink: Send + Sync {
    /// Records one line of progress output (stdout or stderr wrapper) for UI or diagnostics.
    fn backup_progress_line(&self, line: String);
}

/// Shared handle passed into async rsync workers (cloneable across stdout/stderr pump tasks).
pub type SharedProgress = Arc<dyn ProgressSink + Send + Sync>;

/// Test helper: accumulate emitted lines into an in-memory buffer for assertions.
#[derive(Clone, Default)]
pub struct CollectLines {
    pub lines: Arc<Mutex<Vec<String>>>,
}

impl CollectLines {
    /// Wraps this collector as a [`SharedProgress`] sink for backup helpers.
    pub fn into_shared(self) -> SharedProgress {
        Arc::new(self)
    }
}

impl ProgressSink for CollectLines {
    fn backup_progress_line(&self, line: String) {
        self.lines.lock().unwrap().push(line);
    }
}
