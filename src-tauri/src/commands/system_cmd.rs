/*
 * Read-only system metadata for the laptop dashboard (hostname, OS, kernel).
 *
 * Thin IPC proxy: delegates to the backrd daemon so system info is always gathered
 * in the same process context as the daemon.  The function signature is kept identical
 * to preserve the frontend `invoke()` call contract.
 */

use serde::{Deserialize, Serialize};

use crate::error::BackrCommandError;

/// Snapshot of local machine facts rendered beside backup tooling.
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub hostname: Option<String>,
    /// Distro pretty line (`PRETTY_NAME`) when `/etc/os-release` exists; otherwise OS family string.
    pub os_pretty: String,
    /// Kernel release from `uname -r` when available (typically Unix).
    pub kernel_release: Option<String>,
    /// rustc/`std` target architecture token (`ARCH`).
    pub arch: String,
    /// Effective username (`USER` / `USERNAME`).
    pub user: Option<String>,
    /// RFC3339 local timestamp taken when this snapshot was built on the Rust side.
    pub sampled_at_rfc3339: String,
}

/// Collects hostname, distro label, kernel, arch, user, and a sample wall-clock instant from the daemon.
///
/// # Returns
///
/// [`SystemInfo`] populated by the daemon's environment.
#[tauri::command]
pub async fn get_system_info() -> Result<SystemInfo, BackrCommandError> {
    let v = crate::ipc_client::send("get_system_info", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize system info: {e}"))
    })
}
