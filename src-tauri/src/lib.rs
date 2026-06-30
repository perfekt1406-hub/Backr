/*
 * Library entry: wires configuration loading, daemon connectivity, and commands.
 *
 * The scheduler and rsync logic now live in the backrd daemon.  This file bootstraps the
 * Tauri window, verifies that the daemon socket is reachable (spawning backrd if needed),
 * and registers the IPC proxy commands.  The system tray belongs to the always-resident
 * backrd daemon, not the GUI: backr-app is an ephemeral window that exits when closed and
 * reopens fresh on the next launch (or via the daemon tray's "Open Backr").
 *
 * Exposes [`run()`] consumed by the small `main.rs` binary wrapper.
 *
 * Business-logic modules now live in `backr_core` (the `crates/backr-core` crate).
 * Only Tauri-coupled modules remain here: commands, state, and the Tauri-specific
 * parts of the progress sink and scheduler wiring.
 */

pub mod commands;
pub mod ipc_client;
pub mod progress_sink;
pub mod scheduler;
pub mod state;

// Re-export backr_core modules so existing command code can use short paths where needed.
// (Commands currently still use `crate::config`, `crate::backup`, etc. via re-exports.)
pub use backr_core::backup;
pub use backr_core::config;
pub use backr_core::error;
pub use backr_core::host_config;
pub use backr_core::host_disk_inventory;
pub use backr_core::host_trust;
pub use backr_core::pairing;
pub use backr_core::progress_sink as progress_sink_core;
pub use backr_core::project_snapshot_cache;

use std::sync::Arc;

use tauri::Manager;

/// Attempts to connect to the backrd socket; if unreachable, tries to spawn backrd once
/// and waits briefly before probing again.
///
/// This is best-effort: if the daemon is still not reachable after one spawn attempt the
/// function returns `Err` with a human-readable message.  The GUI window still opens so the
/// user can see the error rather than getting a silent crash.
///
/// # Returns
///
/// `Ok(())` when a connection to the daemon socket succeeds; `Err(String)` otherwise.
async fn ensure_daemon_running() -> Result<(), String> {
    let path = ipc_client::socket_path();

    // First probe: is the daemon already listening?
    if tokio::net::UnixStream::connect(&path).await.is_ok() {
        return Ok(());
    }

    // Daemon not reachable — try to spawn it.
    tracing::info!("backrd socket not found at {}; attempting to spawn backrd", path.display());
    let spawn_result = tokio::process::Command::new("backrd").spawn();
    match spawn_result {
        Ok(_) => {
            tracing::info!("backrd spawned; waiting 500 ms for it to bind");
        }
        Err(e) => {
            tracing::warn!("could not spawn backrd: {e}");
        }
    }

    // Allow time for backrd to create the socket.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Second probe: did the daemon start in time?
    if tokio::net::UnixStream::connect(&path).await.is_ok() {
        return Ok(());
    }

    Err(format!(
        "backrd daemon is not running and could not be started (socket: {})",
        path.display()
    ))
}

/// Bootstraps tracing, plugins, managed state, and the GTK/WebKit window host.
///
/// # Panics
///
/// Panics when the Tauri event loop fails to start (fatal for desktop shells).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Must run before tauri::Builder — applies WebKitGTK rendering workarounds.
    //
    // WEBKIT_DISABLE_DMABUF_RENDERER=1 prevents blank/white windows caused by
    // DMA-BUF framebuffer failures.  Originally NVIDIA-only, but the same symptom
    // appears on AMD/Intel GPUs when WebKitGTK and Mesa versions drift apart —
    // especially on rolling-release distros (Arch, Manjaro, CachyOS …).
    //
    // Setting the var unconditionally on Wayland is safe: the fallback renderer
    // uses shared-memory buffers with no user-visible difference for a utility app.
    // We still gate __NV_DISABLE_EXPLICIT_SYNC on actual NVIDIA presence.
    #[cfg(target_os = "linux")]
    {
        let on_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v == "wayland")
                .unwrap_or(false);

        // DMA-BUF workaround: always on Wayland, or when NVIDIA is detected on X11.
        let nvidia_present = webkit2gtk_nvidia_quirk::is_primary_gpu_nvidia()
            || std::path::Path::new("/sys/module/nvidia").exists();
        if on_wayland || nvidia_present {
            webkit2gtk_nvidia_quirk::set_webkit_disable_dmabuf_renderer();
        }

        if nvidia_present {
            webkit2gtk_nvidia_quirk::nv_disable_explicit_sync();
        }
    }

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second launch should reveal the existing instance.  The window may
            // be minimized, so show + unminimize before focusing.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .manage(Arc::new(state::AppState::default()))
        .setup(|app| {
            let handle = app.handle().clone();
            let state: Arc<state::AppState> =
                handle.state::<Arc<state::AppState>>().inner().clone();

            tauri::async_runtime::block_on(async {
                // Check daemon connectivity before proceeding; store the error in state so
                // the frontend can surface a clear error screen via `get_daemon_error`.
                if let Err(err) = ensure_daemon_running().await {
                    tracing::error!("daemon unreachable: {err}");
                    let mut slot = state.daemon_error.lock().await;
                    *slot = Some(err);
                    // Continue anyway so the window opens — the frontend will display the error.
                }
            });

            // Auto-update tick (fire-and-forget): poke the daemon to consider applying an
            // update at launch. A no-op when auto-update is off or the throttle window
            // hasn't elapsed; the daemon runs the check + worker launch in the background,
            // so this never delays the window opening.
            tauri::async_runtime::spawn(async {
                let _ = crate::ipc_client::send("auto_update_tick", serde_json::json!({})).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::activity_cmd::get_activity_series,
            commands::config_cmd::get_config,
            commands::config_cmd::save_config,
            commands::config_cmd::test_connection,
            commands::host_cmd::resolve_shell_bootstrap,
            commands::host_cmd::host_list_snapshot_projects,
            commands::host_cmd::host_volume_summary,
            commands::host_cmd::host_disk_inventory,
            commands::host_cmd::host_trust_status,
            commands::host_cmd::host_append_authorized_pubkey,
            commands::host_cmd::host_list_authorized_pubkeys,
            commands::host_cmd::host_remove_authorized_pubkey,
            commands::pairing_cmd::start_pairing,
            commands::pairing_cmd::stop_pairing,
            commands::pairing_cmd::pairing_status,
            commands::pairing_cmd::discover_hosts,
            commands::pairing_cmd::pair_with_host,
            commands::pairing_cmd::confirm_pairing,
            commands::system_cmd::get_system_info,
            commands::project_cmd::list_projects,
            commands::project_cmd::get_backup_status,
            commands::backup_cmd::run_backup,
            commands::snapshot_cmd::list_snapshots,
            commands::snapshot_cmd::list_files,
            commands::snapshot_cmd::read_snapshot_file,
            commands::snapshot_cmd::restore_snapshot,
            commands::snapshot_cmd::restore_all_snapshots,
            commands::snapshot_cmd::restore_all_projects,
            commands::update_cmd::get_update_status,
            commands::update_cmd::get_update_settings,
            commands::update_cmd::set_update_settings,
            commands::update_cmd::apply_update,
            get_daemon_error,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Returns any daemon connectivity error recorded during startup, or `null` when the daemon is healthy.
///
/// The frontend calls this on load to determine whether to show an error screen instead of
/// the normal dashboard.  Returns `None` (serialised as `null`) when the daemon started cleanly.
///
/// # Returns
///
/// `Ok(Some(message))` when the daemon was unreachable; `Ok(None)` when all is well.
#[tauri::command]
async fn get_daemon_error(
    state: tauri::State<'_, Arc<state::AppState>>,
) -> Result<Option<String>, error::BackrCommandError> {
    Ok(state.daemon_error.lock().await.clone())
}
