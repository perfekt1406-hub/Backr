/*
 * Periodic backup scheduler built on Tokio timers and cancellation tokens.
 * Restarts cleanly whenever configuration is saved so interval edits apply immediately.
 */

use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Stops any existing scheduler, then (if configured) starts a new sleeping loop.
///
/// # Inputs
///
/// * `app` — global handle forwarded to scheduled backup jobs.
/// * `state` — shared [`AppState`] carrying join handles and tokens.
///
/// # Returns
///
/// `Ok` after the replacement completes; errors are surfaced as plain strings for setup code.
pub async fn restart_scheduler(app: &AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    {
        let mut slot = state.scheduler_cancel.lock().await;
        if let Some(old) = slot.take() {
            old.cancel();
        }
    }
    {
        let mut slot = state.scheduler_handle.lock().await;
        if let Some(handle) = slot.take() {
            handle.abort();
        }
    }

    let cfg = state.config.lock().await.clone();
    let Some(cfg) = cfg else {
        return Ok(());
    };

    let period_secs = (cfg.schedule.interval_hours as u64)
        .saturating_mul(3600)
        .max(60);
    let period = Duration::from_secs(period_secs);

    let token = CancellationToken::new();
    {
        let mut slot = state.scheduler_cancel.lock().await;
        *slot = Some(token.clone());
    }

    let app_cl = app.clone();
    let st = Arc::clone(state);
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
              _ = token.cancelled() => break,
              _ = tokio::time::sleep(period) => {
                if let Err(err) =
                  crate::commands::backup_cmd::run_scheduled_backup(app_cl.clone()).await
                {
                  tracing::warn!("scheduler tick failed: {err}");
                }
                let _ = crate::tray::update_tooltip(&app_cl, &st).await;
              }
            }
        }
    });

    {
        let mut slot = state.scheduler_handle.lock().await;
        *slot = Some(join);
    }

    Ok(())
}
