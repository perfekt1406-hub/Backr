/*
 * Backup command surface: on-demand backup runs proxied to the backrd daemon.
 *
 * All Tauri-command functions are thin IPC proxies.  `execute_backup_cycle_with_sink`
 * and `spawn_backup_job` are retained as public items because they are re-exported from
 * `commands/mod.rs` and referenced elsewhere; they now delegate to the daemon rather than
 * running rsync directly.
 *
 * Commands that stream rsync progress use `send_with_progress` so the daemon's
 * `backup_progress` side-channel events are re-emitted to the webview via the standard
 * `backup://progress` Tauri event.
 */

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::backup::rsync::BACKUP_PROGRESS_EVENT;
use crate::error::{BackrCommandError, BackrError};
use crate::progress_sink::SharedProgress;
use crate::state::AppState;

/// Stub retained for API compatibility with tests that inject a custom [`SharedProgress`] sink.
///
/// In the daemon-GUI split model the daemon owns all rsync orchestration; this function now
/// sends a `run_backup` request to the daemon and discards streaming progress (tests that need
/// progress lines should target the daemon directly).  Kept `pub` because it is re-exported
/// from `commands/mod.rs`.
///
/// # Inputs
///
/// * `_sink`   — progress sink (ignored in proxy mode; daemon streams events via the socket).
/// * `_state`  — shared application state (unused; daemon owns backup state).
/// * `project` — optional directory name restricting the backup to a single project.
pub async fn execute_backup_cycle_with_sink(
    _sink: SharedProgress,
    _state: &Arc<AppState>,
    project: Option<String>,
) -> Result<(), BackrError> {
    // In proxy mode we cannot forward progress lines to a non-Tauri sink, so we send
    // the request to the daemon and wait for completion.
    crate::ipc_client::send(
        "run_backup",
        serde_json::json!({ "project": project }),
    )
    .await
    .map_err(|e| BackrError::Msg(e.message.to_string()))?;
    Ok(())
}

/// Spawns an asynchronous backup job that proxies to the daemon and returns immediately.
///
/// Retained as a public function because the scheduler (and tests) call it directly.  In the
/// daemon-GUI split model the daemon owns the in-progress guard; the Tauri side simply fires the
/// request and monitors the socket for progress/completion.
///
/// # Inputs
///
/// * `app`     — Tauri app handle used for progress event emission.
/// * `_state`  — shared application state (unused; daemon owns backup state).
/// * `project` — optional directory name restricting the sync to a single project.
///
/// # Returns
///
/// `Ok(())` when the worker was scheduled; `Err(BackrCommandError)` on a spawn failure.
pub fn spawn_backup_job(
    app: &AppHandle,
    _state: &Arc<AppState>,
    project: Option<String>,
) -> Result<(), BackrCommandError> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let res =
            crate::ipc_client::send_with_progress(
                "run_backup",
                serde_json::json!({ "project": project }),
                &app,
            )
            .await;
        if let Err(err) = res {
            let _ = app.emit(
                BACKUP_PROGRESS_EVENT,
                format!("[backr] error: {}", err.message),
            );
        }
    });

    Ok(())
}

/// Tauri command entrypoint: triggers a backup run on the daemon with live progress streaming.
///
/// # Inputs
///
/// * `app`     — Tauri app handle forwarded to [`crate::ipc_client::send_with_progress`].
/// * `state`   — managed application state (unused by the proxy; retained for signature parity).
/// * `project` — optional project directory name to back up (all projects when omitted).
#[tauri::command]
pub async fn run_backup(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    project: Option<String>,
) -> Result<(), BackrCommandError> {
    spawn_backup_job(&app, state.inner(), project)
}

/// Runs one scheduled backup on the daemon (residual scheduler-wiring proxy).
///
/// In daemon-GUI split mode the daemon owns its own scheduler; this function is kept for
/// any residual Tauri-side scheduler wiring that has not yet been removed and simply
/// proxies the call.
///
/// # Inputs
///
/// * `app` — global Tauri handle used to emit progress events.
///
/// # Returns
///
/// `Ok` when the request completes or was skipped by the daemon.
pub async fn run_scheduled_backup(app: AppHandle) -> Result<(), String> {
    crate::ipc_client::send_with_progress(
        "run_backup",
        serde_json::json!({ "project": null }),
        &app,
    )
    .await
    .map_err(|e| e.message.to_string())?;
    Ok(())
}
