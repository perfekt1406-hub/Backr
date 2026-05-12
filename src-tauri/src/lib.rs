/*
 * Library entry: wires configuration loading, tray installation, scheduler startup, and commands.
 * Exposes [`run()`] consumed by the small `main.rs` binary wrapper.
 */

pub mod backup;
pub mod commands;
pub mod config;
pub mod error;
pub mod host_config;
pub mod host_disk_inventory;
pub mod progress_sink;
pub mod project_snapshot_cache;
pub mod scheduler;
pub mod state;
pub mod tray;

use std::sync::Arc;

use tauri::Manager;
use tauri::WindowEvent;

/// Bootstraps tracing, plugins, managed state, and the GTK/WebKit window host.
///
/// # Panics
///
/// Panics when the Tauri event loop fails to start (fatal for desktop shells).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .manage(Arc::new(state::AppState::default()))
        .setup(|app| {
            let handle = app.handle().clone();
            let state: Arc<state::AppState> =
                handle.state::<Arc<state::AppState>>().inner().clone();

            tauri::async_runtime::block_on(async {
                if let Ok(Some(cfg)) = config::load_config() {
                    let mut last = state.last_backup_at.lock().await;
                    *last = cfg.state.last_backup_at;
                    drop(last);
                    let mut guard = state.config.lock().await;
                    *guard = Some(cfg);
                }

                if let Err(err) = scheduler::restart_scheduler(&handle, &state).await {
                    tracing::warn!("failed to start scheduler: {err}");
                }

                if let Err(err) = tray::create_tray(&handle) {
                    tracing::warn!("failed to create tray: {err}");
                }

                let _ = tray::update_tooltip(&handle, &state).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
