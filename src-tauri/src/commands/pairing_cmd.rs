/*
 * Host-side pairing commands.
 *
 * `start_pairing` opens a time-boxed window: generates a 6-digit code, binds an
 * ephemeral pairing listener, advertises this host over mDNS, and auto-tears down
 * after the TTL. `stop_pairing` / `pairing_status` manage and report that window.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::State;
use tiny_http::Server;

use crate::config::Config;
use crate::error::BackrCommandError;
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
pub async fn start_pairing(
    state: State<'_, Arc<AppState>>,
) -> Result<PairingStarted, BackrCommandError> {
    let app = state.inner().clone();
    /* stop_pairing_internal tears down any prior pairing window before opening a new one. */
    stop_pairing_internal(&app).await;

    /* gather_host_info collects hostname, SSH pubkey, and port for the pairing payload. */
    let host = gather_host_info().map_err(BackrCommandError::pairing)?;
    let session = PairingSession::new();
    let code = session.code().to_string();

    /* Server::http binds an ephemeral TCP listener for the pairing HTTP handshake. */
    let server = Server::http("0.0.0.0:0").map_err(|e| BackrCommandError::pairing(e.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or_else(|| BackrCommandError::pairing("could not resolve pairing listener port"))?;
    let server = Arc::new(server);

    /* advertise broadcasts this host over mDNS on the resolved port. */
    let (mdns, fullname) = advertise(port).map_err(BackrCommandError::pairing)?;

    *app.pairing.lock().await = Some(session);

    /* serve runs the pairing HTTP handler on a dedicated OS thread (blocking I/O). */
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
pub async fn stop_pairing(state: State<'_, Arc<AppState>>) -> Result<(), BackrCommandError> {
    stop_pairing_internal(state.inner()).await;
    Ok(())
}

/// True while a pairing window is open.
#[tauri::command]
pub async fn pairing_status(state: State<'_, Arc<AppState>>) -> Result<bool, BackrCommandError> {
    Ok(state.inner().pairing_runtime.lock().await.is_some())
}

/// Browses the LAN for hosts currently in pairing mode (~2.5s window).
#[tauri::command]
pub async fn discover_hosts() -> Result<Vec<DiscoveredHost>, BackrCommandError> {
    /* tokio::task::spawn_blocking runs the mDNS browse on a thread-pool thread (blocking call). */
    tokio::task::spawn_blocking(|| do_discover_hosts(Duration::from_millis(2500)))
        .await
        .map_err(|e| BackrCommandError::task_failed(e.to_string()))?
        .map_err(BackrCommandError::pairing)
}

/// Pairs this laptop with a discovered host using the 6-digit code; returns a
/// prefilled config draft for the setup wizard.
#[tauri::command]
pub async fn pair_with_host(
    address: String,
    code: String,
) -> Result<Config, BackrCommandError> {
    /* tokio::task::spawn_blocking runs the HTTP pairing exchange on a thread-pool thread. */
    tokio::task::spawn_blocking(move || do_pair_with_host(&address, &code))
        .await
        .map_err(|e| BackrCommandError::task_failed(e.to_string()))?
        .map_err(BackrCommandError::pairing)
}

/// Tears down any active pairing runtime and clears the session.
async fn stop_pairing_internal(app: &AppState) {
    let rt = app.pairing_runtime.lock().await.take();
    if let Some(rt) = rt {
        /* PairingRuntime::shutdown stops the mDNS daemon and sends a shutdown signal to the HTTP server. */
        rt.shutdown();
    }
    *app.pairing.lock().await = None;
}
