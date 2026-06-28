/*
 * main.rs — entry point for the backrd daemon binary.
 *
 * Responsibilities:
 *  1. Initialise `tracing-subscriber` for structured logging.
 *  2. Resolve the Unix socket path via `socket_path()` (KTD-2).
 *  3. Create the socket parent directory (mode 0700) and remove any stale
 *     socket file left from a previous run.
 *  4. Bind a `UnixListener` on that path.
 *  5. Allocate `Arc<DaemonState>`.
 *  6. Accept client connections in a loop, spawning one Tokio task per
 *     connection via `ipc::handle_connection`.
 *
 * The scheduler (U4) and tray integration (U6) will be wired in here in
 * subsequent units; for now the daemon is a pure IPC server.
 */

mod daemon_state;
mod ipc;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::net::UnixListener;
use tracing::{error, info};

use daemon_state::DaemonState;

/// Returns the Unix socket path, preferring `$XDG_RUNTIME_DIR/backr/backrd.sock`
/// and falling back to `~/.local/share/backr/backrd.sock` (KTD-2).
///
/// The returned path is always absolute and resides under a directory that the
/// current user owns, avoiding world-accessible locations like `/tmp`.
fn socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("backr").join("backrd.sock")
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".local/share/backr/backrd.sock")
    }
}

/// Creates the parent directory of `path` with mode 0700 on Unix so the socket
/// is not reachable by other users, then removes any stale socket file at `path`.
///
/// # Parameters
/// - `path` — Absolute socket path returned by [`socket_path`].
///
/// # Errors
/// Returns `std::io::Error` if directory creation fails (other than "already exists").
async fn prepare_socket_dir(path: &std::path::Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .expect("socket path must have a parent directory");

    // Create directory with 0700 permissions (owner-only access).
    // `tokio::fs::DirBuilder::mode` is available on Unix via its own DirBuilderExt impl.
    #[cfg(unix)]
    {
        tokio::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .await?;
    }

    // Non-Unix fallback: just create the directory without mode enforcement.
    #[cfg(not(unix))]
    tokio::fs::create_dir_all(parent).await?;

    // Remove stale socket from a previous daemon run so bind() can succeed.
    if path.exists() {
        tokio::fs::remove_file(path).await?;
        info!("removed stale socket at {}", path.display());
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    // Initialise structured logging; RUST_LOG controls verbosity at runtime.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("backrd=info")),
        )
        .init();

    info!("backrd starting");

    // Resolve socket path and prepare the directory.
    let path = socket_path();
    info!("socket path: {}", path.display());

    if let Err(e) = prepare_socket_dir(&path).await {
        error!("failed to prepare socket directory: {e}");
        std::process::exit(1);
    }

    // Bind the Unix listener on the socket path.
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind Unix socket at {}: {e}", path.display());
            std::process::exit(1);
        }
    };

    info!("listening on {}", path.display());

    // Shared daemon state cloned into every connection handler task.
    let state = Arc::new(DaemonState::new());

    // Accept loop: one Tokio task per connection.
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    ipc::handle_connection(stream, state).await;
                });
            }
            Err(e) => {
                // Log and continue rather than crashing on transient errors.
                error!("accept error: {e}");
            }
        }
    }
}
