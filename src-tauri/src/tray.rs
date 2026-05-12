/*
 * System tray wiring: application menu, standard actions, and tooltip updates for backup freshness.
 * Uses Tauri's tray icon builder so Backr stays resident with quick access to backups.
 */

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::commands::backup_cmd::spawn_backup_job;
use crate::state::AppState;

/// Stable identifier used when locating the tray icon for tooltip refreshes.
pub const TRAY_ID: &str = "backr-tray";

/// Installs the tray icon once during application setup.
///
/// # Inputs
///
/// * `app` — fully initialized [`AppHandle`] used to bind menu callbacks.
///
/// # Returns
///
/// `Ok` when the tray initializes; `Err` carries a user-visible string for logging.
pub fn create_tray(app: &AppHandle) -> Result<(), String> {
    let open_i = MenuItem::with_id(app, "open", "Open Backr", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let backup_i = MenuItem::with_id(app, "backup", "Back Up Now", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit_i =
        MenuItem::with_id(app, "quit", "Quit", true, None::<&str>).map_err(|e| e.to_string())?;
    let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;

    let menu =
        Menu::with_items(app, &[&open_i, &backup_i, &sep, &quit_i]).map_err(|e| e.to_string())?;

    let icon = app
        .default_window_icon()
        .ok_or_else(|| "missing default window icon for tray".to_string())?
        .clone();

    let _ = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip("Backr")
        .on_menu_event(move |app, event| {
            let id = event.id.as_ref();
            match id {
                "open" => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                "backup" => {
                    let state = app.state::<Arc<AppState>>();
                    let _ = spawn_backup_job(app, state.inner(), None);
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Updates the tray hover text using the best-known last backup instant.
///
/// # Inputs
///
/// * `state` — shared [`AppState`] containing `last_backup_at`.
pub async fn update_tooltip(app: &AppHandle, state: &Arc<AppState>) -> Result<(), String> {
    let last = *state.last_backup_at.lock().await;
    let text = if let Some(t) = last {
        format!("Backr — last backup: {}", t.format("%Y-%m-%d %H:%M UTC"))
    } else {
        "Backr — last backup: never".into()
    };

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_tooltip(Some(text)).map_err(|e| e.to_string())?;
    }

    Ok(())
}
