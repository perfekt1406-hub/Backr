/*
 * tray.rs — Linux system tray integration for the backrd daemon.
 *
 * Provides a StatusNotifierItem tray icon (via the `ksni` crate, pure-Rust,
 * no GTK required) that shows the last backup time in its tooltip and exposes
 * four context-menu actions: Open Backr, Back Up Now, a separator, and Quit.
 *
 * The entire module is gated with `#[cfg(target_os = "linux")]`.  Non-Linux
 * targets compile to lightweight no-op stubs so that `main.rs` and
 * `scheduler.rs` can call `tray::spawn_tray` / `tray::update_label`
 * unconditionally without platform guards at each call site.
 */

// ---------------------------------------------------------------------------
// Linux implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod inner {
    use std::sync::{Arc, Mutex};

    use tokio::sync::{broadcast, mpsc};

    use backr_core::scheduler::BackupTrigger as _;

    use crate::daemon_state::DaemonState;
    use crate::ipc::protocol::IpcEvent;
    use crate::scheduler::DaemonBackupTrigger;

    // -----------------------------------------------------------------------
    // BackrdTray — ksni::Tray implementation
    // -----------------------------------------------------------------------

    /// Internal tray model; lives on the ksni service thread (an OS thread,
    /// not a Tokio worker).
    ///
    /// `backup_tx` is an unbounded mpsc sender whose receiver lives in a
    /// Tokio task.  Sending on this channel is the only non-blocking,
    /// thread-safe way to trigger a Tokio async operation from a `Fn` closure
    /// that lives on a different OS thread.
    struct BackrdTray {
        /// Human-readable summary of the most recently completed backup,
        /// e.g. `"last backup: 14:32 UTC"` or `"never backed up"`.
        last_backup_label: String,
        /// Signals the Tokio listener task to start an immediate backup.
        backup_tx: mpsc::UnboundedSender<()>,
    }

    impl ksni::Tray for BackrdTray {
        /// Stable, unique identifier required by some tray implementations
        /// (e.g. KDE Plasma) to avoid duplicate entries.
        fn id(&self) -> String {
            "backrd".into()
        }

        /// Tooltip title shown by the desktop shell on hover.
        fn title(&self) -> String {
            format!("Backr — {}", self.last_backup_label)
        }

        /// Freedesktop icon name; falls back gracefully when the theme does
        /// not ship this icon.
        fn icon_name(&self) -> String {
            "drive-harddisk-symbolic".into()
        }

        /// Context menu rendered when the user right-clicks the tray icon.
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            vec![
                // Launch the Backr GUI frontend.
                StandardItem {
                    label: "Open Backr".into(),
                    activate: Box::new(|_this: &mut Self| {
                        // Non-fatal if the binary isn't installed; the daemon
                        // continues running regardless.
                        let _ = std::process::Command::new("backr-app").spawn();
                    }),
                    ..Default::default()
                }
                .into(),
                // Request an immediate out-of-schedule backup.
                StandardItem {
                    label: "Back Up Now".into(),
                    activate: Box::new(|this: &mut Self| {
                        // Sends on the channel; the Tokio listener task does
                        // the actual work so we don't block the ksni thread.
                        let _ = this.backup_tx.send(());
                    }),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                // Terminate the daemon process.
                StandardItem {
                    label: "Quit".into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|_this: &mut Self| std::process::exit(0)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    // -----------------------------------------------------------------------
    // Global tray handle (populated once by spawn_tray)
    // -----------------------------------------------------------------------

    /// Global slot holding the `ksni::Handle` after `spawn_tray` is called.
    ///
    /// `Mutex<Option<...>>` so it can be written once from the caller thread
    /// and subsequently read from Tokio tasks via `update_label`.
    static TRAY_HANDLE: Mutex<Option<ksni::Handle<BackrdTray>>> = Mutex::new(None);

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Spawns the ksni tray service and wires the "Back Up Now" listener task.
    ///
    /// 1. Creates a `BackrdTray` initialised with the current last-backup label.
    /// 2. Wraps it in a `ksni::TrayService`, retains the `Handle` for later
    ///    label updates, and spawns the service on a new OS thread (ksni uses
    ///    a blocking D-Bus event loop).
    /// 3. Spawns a Tokio task that listens for "Back Up Now" signals from the
    ///    tray and calls `DaemonBackupTrigger::trigger_backup`.
    ///
    /// # Parameters
    /// - `state`    — Shared daemon state; read for the initial tooltip label
    ///   and passed into the backup trigger.
    /// - `event_tx` — IPC broadcast sender forwarded to the backup trigger so
    ///   manual backup tasks can push progress events to clients.
    pub fn spawn_tray(state: Arc<DaemonState>, event_tx: broadcast::Sender<IpcEvent>) {
        // Unbounded channel: ksni menu callback → Tokio backup listener task.
        let (backup_tx, mut backup_rx) = mpsc::unbounded_channel::<()>();

        let initial_label = format_label(&state);
        let tray = BackrdTray {
            last_backup_label: initial_label,
            backup_tx,
        };

        // Build service, grab handle, then spawn the D-Bus loop on an OS thread.
        let service = ksni::TrayService::new(tray);
        let handle = service.handle();
        service.spawn(); // non-blocking; spawns a std::thread internally

        // Store the handle so update_label can reach it.
        {
            let mut guard = TRAY_HANDLE.lock().expect("tray handle mutex poisoned");
            *guard = Some(handle);
        }

        // Backup trigger shared with the listener task.
        let trigger = Arc::new(DaemonBackupTrigger::new(Arc::clone(&state), event_tx));

        // Tokio task: wait for "Back Up Now" signals and execute the backup.
        tokio::spawn(async move {
            while backup_rx.recv().await.is_some() {
                /* DaemonBackupTrigger::trigger_backup spawns an async task that runs the
                   full rsync backup pipeline and broadcasts progress events to GUI clients. */
                trigger.trigger_backup();
            }
        });
    }

    /// Refreshes the tray tooltip with the current `last_backup_at` value from
    /// `DaemonState`.  Called after each successful backup tick.
    ///
    /// Reads the timestamp via `format_label` (which uses a non-blocking
    /// `try_lock`, safe to call from any thread).  The `ksni::Handle::update`
    /// call is itself synchronous and non-blocking from the caller's
    /// perspective (it queues a D-Bus update on the service thread).
    ///
    /// # Parameters
    /// - `state` — Shared daemon state; `last_backup_at` is read to format the
    ///   new label string.
    pub fn update_label(state: &DaemonState) {
        let label = format_label(state);
        let guard = TRAY_HANDLE.lock().expect("tray handle mutex poisoned");
        if let Some(handle) = guard.as_ref() {
            handle.update(|tray: &mut BackrdTray| {
                tray.last_backup_label = label.clone();
            });
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Formats a human-readable last-backup label from daemon state.
    ///
    /// Returns `"never backed up"` when no backup has yet completed, or
    /// `"last backup: HH:MM UTC"` with the UTC hour and minute otherwise.
    ///
    /// Uses `try_lock` rather than `blocking_lock`: this runs on the Tokio main
    /// thread at startup (`spawn_tray`) and on worker threads after a backup
    /// (`update_label`), and `blocking_lock` panics ("cannot block the current
    /// thread from within a runtime") when called inside the Tokio runtime.
    /// The lock only ever guards a brief timestamp read/write, so it is
    /// effectively always uncontended here; a rare contended read falls back to
    /// the "never backed up" label and self-heals on the next update.
    ///
    /// # Parameters
    /// - `state` — Shared daemon state; reads `last_backup_at`.
    ///
    /// # Returns
    /// A `String` suitable for display in the tray tooltip.
    fn format_label(state: &DaemonState) -> String {
        let last = state.last_backup_at.try_lock().ok().and_then(|g| *g);
        match last {
            None => "never backed up".into(),
            Some(ts) => format!("last backup: {}", ts.format("%H:%M UTC")),
        }
    }
}

// ---------------------------------------------------------------------------
// Re-exports (Linux only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub use inner::{spawn_tray, update_label};

// ---------------------------------------------------------------------------
// No-op stubs for non-Linux platforms
// ---------------------------------------------------------------------------

/// No-op stub: on non-Linux targets `backrd` runs headlessly with no tray.
#[cfg(not(target_os = "linux"))]
pub fn spawn_tray(
    _state: std::sync::Arc<crate::daemon_state::DaemonState>,
    _event_tx: tokio::sync::broadcast::Sender<crate::ipc::protocol::IpcEvent>,
) {
}

/// No-op stub: on non-Linux targets label updates are silently discarded.
#[cfg(not(target_os = "linux"))]
pub fn update_label(_state: &crate::daemon_state::DaemonState) {}
