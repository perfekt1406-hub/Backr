/*
 * Tauri-specific progress sink implementation.
 *
 * The portable `ProgressSink` trait and `CollectLines` test helper live in
 * `backr_core::progress_sink`.  This file contains only `AppEmitProgress`, which
 * forwards progress lines to the Tauri webview via `AppHandle::emit`.  It cannot
 * move to `backr_core` because it depends on `tauri::AppHandle`.
 */

use tauri::{AppHandle, Emitter};

use backr_core::backup::rsync::BACKUP_PROGRESS_EVENT;
use backr_core::progress_sink::ProgressSink;

// Re-export portable types from backr_core so existing command code can keep its imports.
pub use backr_core::progress_sink::{CollectLines, SharedProgress};

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
