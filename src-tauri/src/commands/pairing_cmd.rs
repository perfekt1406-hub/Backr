/*
 * Host-side pairing commands.
 *
 * `start_pairing` opens a time-boxed window: generates a 6-digit code, binds an
 * ephemeral pairing listener, advertises this host over mDNS, and auto-tears down
 * after the TTL. `stop_pairing` / `pairing_status` manage and report that window.
 */

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::State;
use tiny_http::Server;

use crate::config::Config;
use crate::pairing::client::pair_with_host as do_pair_with_host;
use crate::pairing::code::PairingSession;
use crate::pairing::discovery::{advertise, discover_hosts as do_discover_hosts, DiscoveredHost};
use crate::pairing::listener::{gather_host_info, serve};
use crate::pairing::PairingRuntime;
use crate::state::AppState;

/// Returned to the host UI when a pairing window opens.
#[derive(serde::Serialize)]
pub struct PairingStarted {
    /// 6-digit code to show on the host.
    pub code: String,
}

/// Opens a pairing window (code + mDNS advertise + listener) that stays open until a
/// laptop pairs or stop_pairing is called.
#[tauri::command]
pub async fn start_pairing(state: State<'_, Arc<AppState>>) -> Result<PairingStarted, String> {
    let app = state.inner().clone();
    // Replace any prior window.
    stop_pairing_internal(&app).await;

    let host = gather_host_info()?;
    let session = PairingSession::new();
    let code = session.code().to_string();

    // Bind the listener on an ephemeral port, then advertise that exact port.
    let server = Server::http("0.0.0.0:0").map_err(|e| e.to_string())?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or_else(|| "could not resolve pairing listener port".to_string())?;
    let server = Arc::new(server);

    let (mdns, fullname) = advertise(port)?;

    *app.pairing.lock().await = Some(session);

    // Serve runs on a dedicated OS thread (it uses blocking_lock).
    let serve_app = app.clone();
    let serve_server = server.clone();
    let serve_host = host.clone();
    let handle = thread::spawn(move || serve(serve_server, serve_app, serve_host));

    *app.pairing_runtime.lock().await = Some(PairingRuntime {
        mdns,
        fullname,
        server,
        thread: Some(handle),
    });

    Ok(PairingStarted { code })
}

/// Closes the pairing window if one is open.
#[tauri::command]
pub async fn stop_pairing(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    stop_pairing_internal(state.inner()).await;
    Ok(())
}

/// True while a pairing window is open.
#[tauri::command]
pub async fn pairing_status(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(state.inner().pairing_runtime.lock().await.is_some())
}

/// Browses the LAN for hosts currently in pairing mode (~2.5s window).
#[tauri::command]
pub async fn discover_hosts() -> Result<Vec<DiscoveredHost>, String> {
    tokio::task::spawn_blocking(|| do_discover_hosts(Duration::from_millis(2500)))
        .await
        .map_err(|e| e.to_string())?
}

/// Pairs this laptop with a discovered host using the 6-digit code; returns a
/// prefilled config draft for the setup wizard.
#[tauri::command]
pub async fn pair_with_host(address: String, code: String) -> Result<Config, String> {
    tokio::task::spawn_blocking(move || do_pair_with_host(&address, &code))
        .await
        .map_err(|e| e.to_string())?
}

/// Tears down any active pairing runtime and clears the session.
async fn stop_pairing_internal(app: &AppState) {
    let rt = app.pairing_runtime.lock().await.take();
    if let Some(rt) = rt {
        rt.shutdown();
    }
    *app.pairing.lock().await = None;
}
