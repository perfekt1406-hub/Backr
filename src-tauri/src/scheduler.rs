/*
 * Periodic backup scheduler built on Tokio timers and cancellation tokens.
 * Restarts cleanly whenever configuration is saved so interval edits apply immediately.
 *
 * On startup the scheduler computes how long to wait before the first backup fires by
 * comparing `last_backup_at` from persisted config against the current wall clock.
 * If a backup was overdue (elapsed >= interval) it fires promptly; otherwise it waits
 * only the remaining time.  Subsequent ticks use the full interval as before.
 */

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Minimum number of seconds to wait before the very first scheduled backup, even when
/// `last_backup_at` is `None` or the backup is technically overdue.  Gives the app a
/// moment to finish initialising before kicking off I/O.
const MIN_INITIAL_DELAY_SECS: u64 = 60;

/// Computes how long to wait before firing the first scheduled backup.
///
/// Rules (applied in order):
/// 1. If `last` is `None` (never backed up), returns `MIN_INITIAL_DELAY_SECS` so the
///    first backup fires shortly after setup rather than at a full interval.
/// 2. If the last backup is in the *future* (clock skew / time-zone change), returns
///    the full `interval` as a safe fallback — no panic, no negative duration.
/// 3. If elapsed time since the last backup is **≥ interval** (overdue), returns
///    `Duration::ZERO` so the scheduler fires immediately on startup.
/// 4. Otherwise returns `interval - elapsed`, i.e. the remaining time to wait.
/// 5. The result is always at least `MIN_INITIAL_DELAY_SECS` (60 s floor) to give the
///    app a brief settle window, **except** when the backup is genuinely overdue (rule 3),
///    where zero is returned so it can fire straight away.
///
/// # Inputs
///
/// * `last`     — timestamp of the previous successful backup, or `None`.
/// * `interval` — configured backup cadence (already clamped to ≥ 60 s by the caller).
/// * `now`      — current UTC instant, injected for testability.
///
/// # Returns
///
/// A `Duration` in `[0, interval]`.
pub fn compute_initial_delay(
    last: Option<DateTime<Utc>>,
    interval: Duration,
    now: DateTime<Utc>,
) -> Duration {
    let Some(last_ts) = last else {
        // Never backed up — fire after the short settle window.
        return Duration::from_secs(MIN_INITIAL_DELAY_SECS);
    };

    // Compute elapsed time; guard against a last_backup_at that is in the future
    // (e.g. clock skew), which would produce a negative chrono::Duration.
    let elapsed_chrono = now.signed_duration_since(last_ts);
    if elapsed_chrono.num_seconds() < 0 {
        // Clock skew: last timestamp is ahead of now — wait a full interval.
        tracing::warn!(
            "last_backup_at ({last_ts}) is in the future; using full interval as initial delay"
        );
        return interval;
    }

    // Safe to convert: elapsed is non-negative and fits in u64 easily.
    let elapsed = Duration::from_secs(elapsed_chrono.num_seconds() as u64);

    if elapsed >= interval {
        // Backup is overdue — fire immediately (no 60 s floor).
        Duration::ZERO
    } else {
        // Remaining time until next scheduled backup.  Apply the 60 s floor so the
        // app has a brief settle window even if the backup is nearly due.
        let remaining = interval - elapsed;
        remaining.max(Duration::from_secs(MIN_INITIAL_DELAY_SECS))
    }
}

/// Stops any existing scheduler, then (if configured) starts a new sleeping loop.
///
/// The first sleep uses `compute_initial_delay` so a backup that was overdue while the
/// app was closed fires promptly instead of waiting the full interval again.
///
/// # Inputs
///
/// * `app`   — global handle forwarded to scheduled backup jobs.
/// * `state` — shared [`AppState`] carrying join handles and tokens.
///
/// # Returns
///
/// `Ok` after the replacement completes; errors are surfaced as plain strings for setup code.
pub async fn restart_scheduler(app: &AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    // Cancel the currently-running loop (if any).
    {
        let mut slot = state.scheduler_cancel.lock().await;
        if let Some(old) = slot.take() {
            old.cancel();
        }
    }
    // Abort the Tokio task handle so it is cleaned up promptly.
    {
        let mut slot = state.scheduler_handle.lock().await;
        if let Some(handle) = slot.take() {
            handle.abort();
        }
    }

    let cfg = state.config.lock().await.clone();
    let Some(cfg) = cfg else {
        // No configuration yet — nothing to schedule.
        return Ok(());
    };

    // Clamp the user-configured interval to at least 60 seconds to avoid a runaway loop.
    let period_secs = (cfg.schedule.interval_hours as u64)
        .saturating_mul(3600)
        .max(60);
    let period = Duration::from_secs(period_secs);

    // Read the persisted last-backup timestamp so we can compute the catch-up delay.
    let last_backup_at = cfg.state.last_backup_at;

    let token = CancellationToken::new();
    {
        let mut slot = state.scheduler_cancel.lock().await;
        *slot = Some(token.clone());
    }

    let app_cl = app.clone();
    let st = Arc::clone(state);
    let join = tokio::spawn(async move {
        // Compute how long to wait before the first tick, taking into account the time
        // elapsed since the last backup.  This is the key catch-up logic: if the app
        // was closed during a backup window the backup fires promptly on next launch.
        let initial_delay = compute_initial_delay(last_backup_at, period, Utc::now());

        // First sleep: use the computed initial delay (may be zero for overdue backups).
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(initial_delay) => {}
        }

        // Run the first backup tick.
        if let Err(err) =
            crate::commands::backup_cmd::run_scheduled_backup(app_cl.clone()).await
        {
            tracing::warn!("scheduler tick failed: {err}");
        }
        let _ = crate::tray::update_tooltip(&app_cl, &st).await;

        // Steady-state loop: sleep the full interval between subsequent backups.
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

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Helper: build a `DateTime<Utc>` from an offset in seconds relative to `now`.
    fn ts_ago(now: DateTime<Utc>, secs: i64) -> DateTime<Utc> {
        now - chrono::Duration::seconds(secs)
    }

    /// 4 h ago, 3 h interval → overdue → Duration::ZERO.
    #[test]
    fn overdue_returns_zero() {
        let now = Utc::now();
        let interval = Duration::from_secs(3 * 3600);
        let last = ts_ago(now, 4 * 3600);
        assert_eq!(compute_initial_delay(Some(last), interval, now), Duration::ZERO);
    }

    /// Exactly `interval` ago → exactly at the boundary → Duration::ZERO.
    #[test]
    fn exactly_at_boundary_returns_zero() {
        let now = Utc::now();
        let interval = Duration::from_secs(3 * 3600);
        let last = ts_ago(now, 3 * 3600);
        assert_eq!(compute_initial_delay(Some(last), interval, now), Duration::ZERO);
    }

    /// 1 h ago, 3 h interval → ~2 h remaining (clamped to max(remaining, 60 s)).
    #[test]
    fn partial_elapsed_returns_remaining() {
        let now = Utc::now();
        let interval = Duration::from_secs(3 * 3600);
        let last = ts_ago(now, 3600); // 1 h ago
        let delay = compute_initial_delay(Some(last), interval, now);
        // Remaining should be close to 2 h (7200 s), well above the 60 s floor.
        let expected = Duration::from_secs(2 * 3600);
        // Allow ±2 s for test execution time.
        assert!(delay >= expected - Duration::from_secs(2));
        assert!(delay <= expected + Duration::from_secs(2));
    }

    /// Never backed up (`last = None`) → returns short delay (≥ 60 s, ≤ full interval).
    #[test]
    fn never_backed_up_returns_short_delay() {
        let now = Utc::now();
        let interval = Duration::from_secs(3 * 3600);
        let delay = compute_initial_delay(None, interval, now);
        assert!(delay >= Duration::from_secs(MIN_INITIAL_DELAY_SECS));
        assert!(delay <= interval);
    }

    /// `last` is in the future (clock skew) → returns full interval, no panic.
    #[test]
    fn clock_skew_future_timestamp_returns_full_interval() {
        let now = Utc::now();
        let interval = Duration::from_secs(3 * 3600);
        // Simulate a last_backup_at 30 minutes ahead of now.
        let future_ts = now + chrono::Duration::minutes(30);
        let delay = compute_initial_delay(Some(future_ts), interval, now);
        assert_eq!(delay, interval);
    }

    /// Very small interval (0 s effective → clamped to 60 s by caller, but let's test
    /// that compute_initial_delay itself doesn't panic on tiny intervals).
    /// When last is 5 s ago and interval is 10 s, it's overdue → Duration::ZERO.
    #[test]
    fn tiny_interval_overdue() {
        let now = Utc::now();
        let interval = Duration::from_secs(10);
        let last = ts_ago(now, 15); // 15 s ago > 10 s interval
        assert_eq!(compute_initial_delay(Some(last), interval, now), Duration::ZERO);
    }

    /// When remaining time is very small but positive (e.g. 30 s), the 60 s floor applies.
    #[test]
    fn small_remaining_clamped_to_floor() {
        let now = Utc::now();
        let interval = Duration::from_secs(3 * 3600);
        // 30 s short of the full interval → remaining ≈ 30 s → clamped to 60 s.
        let last = ts_ago(now, 3 * 3600 - 30);
        let delay = compute_initial_delay(Some(last), interval, now);
        assert!(delay >= Duration::from_secs(MIN_INITIAL_DELAY_SECS));
    }
}
