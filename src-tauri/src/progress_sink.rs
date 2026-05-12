/*
 * Abstraction for streaming rsync / backup status lines to either the Tauri webview or test collectors.
 * Keeps `backup::rsync` usable from integration tests without a running `AppHandle`.
 */

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::backup::rsync::BACKUP_PROGRESS_EVENT;

/// Receives human-readable rsync progress or status lines (mirrors `backup://progress` payloads).
pub trait ProgressSink: Send + Sync {
    /// Records one line of progress output (stdout or stderr wrapper) for UI or diagnostics.
    fn backup_progress_line(&self, line: String);
}

/// Shared handle passed into async rsync workers (cloneable across stdout/stderr pump tasks).
pub type SharedProgress = Arc<dyn ProgressSink + Send + Sync>;

/// Forwards lines to the real Tauri event bus used by the desktop UI.
pub struct AppEmitProgress(pub AppHandle);

impl AppEmitProgress {
    /// Wraps an [`AppHandle`] so it can be used anywhere a [`ProgressSink`] is required.
    ///
    /// # Inputs
    ///
    /// * `app` — live application handle from the running Tauri runtime.
    pub fn new(app: AppHandle) -> Self {
        Self(app)
    }
}

impl ProgressSink for AppEmitProgress {
    fn backup_progress_line(&self, line: String) {
        let _ = self.0.emit(BACKUP_PROGRESS_EVENT, line);
    }
}

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
