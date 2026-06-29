/*
 * Tauri commands for backup-host dashboard bootstrap plus local filesystem introspection.
 *
 * Thin IPC proxies delegating to the backrd daemon.  The function signatures are
 * kept identical to preserve the frontend `invoke()` call contract.
 *
 * All commands return `Result<T, BackrCommandError>` so the frontend receives a typed
 * `{ kind, message }` error object rather than a bare string.
 */

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::BackrCommandError;

/// JSON payload consumed by `resolve_shell_bootstrap` — determines initial router destination.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ShellBootstrap {
    Setup,
    Client,
    Host {
        backup_root: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        ssh_user: Option<String>,
    },
}

/// One project folder visible under `backup_root` with coarse snapshot stats from disk.
#[derive(Debug, Serialize, Deserialize)]
pub struct HostProjectRow {
    pub name: String,
    pub snapshot_count: usize,
    pub last_backup_at: Option<DateTime<Utc>>,
    /// Newest snapshot directory names (up to three), sorted newest-first.
    pub recent_snapshots: Vec<String>,
}

/// Volume summary for the host dashboard chrome.
#[derive(Debug, Serialize, Deserialize)]
pub struct HostVolumeSummary {
    pub backup_root: String,
    pub bytes_avail: Option<u64>,
    pub bytes_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<String>,
}

/// Chooses laptop setup vs client vs NAS-local dashboard before SPA routing completes.
///
/// # Returns
///
/// [`ShellBootstrap`] indicating which view the UI should render first.
#[tauri::command]
pub async fn resolve_shell_bootstrap() -> Result<ShellBootstrap, BackrCommandError> {
    let v =
        crate::ipc_client::send("resolve_shell_bootstrap", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize shell bootstrap: {e}"))
    })
}

/// Lists projects by scanning `backup_root/<project>/<snapshot>/` locally on the NAS machine via the daemon.
///
/// # Inputs
///
/// * `backup_root` — absolute path to the backup storage root on the NAS.
#[tauri::command]
pub async fn host_list_snapshot_projects(
    backup_root: String,
) -> Result<Vec<HostProjectRow>, BackrCommandError> {
    let v = crate::ipc_client::send(
        "host_list_snapshot_projects",
        serde_json::json!({ "backup_root": backup_root }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize host project list: {e}"))
    })
}

/// Builds a volume summary by probing `df` against `backup_root` via the daemon.
///
/// # Inputs
///
/// * `backup_root` — path whose containing filesystem should be queried.
#[tauri::command]
pub async fn host_volume_summary(
    backup_root: String,
) -> Result<HostVolumeSummary, BackrCommandError> {
    let v = crate::ipc_client::send(
        "host_volume_summary",
        serde_json::json!({ "backup_root": backup_root }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize host volume summary: {e}"))
    })
}

/// Runs `du`-backed disk inventory via the daemon.
///
/// # Inputs
///
/// * `backup_root`   — backup tree root to scan.
/// * `force_refresh` — bypass TTL and attempt a fresh `du` pass when true.
#[tauri::command]
pub async fn host_disk_inventory(
    backup_root: String,
    force_refresh: bool,
) -> Result<crate::host_disk_inventory::HostDiskInventory, BackrCommandError> {
    let v = crate::ipc_client::send(
        "host_disk_inventory",
        serde_json::json!({
            "backup_root": backup_root,
            "force_refresh": force_refresh,
        }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize disk inventory: {e}"))
    })
}

/// Reports authorized_keys path + pubkey count for the backup SSH account (host Trust page).
#[tauri::command]
pub async fn host_trust_status() -> Result<crate::host_trust::HostTrustStatus, BackrCommandError> {
    let v =
        crate::ipc_client::send("host_trust_status", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize host trust status: {e}"))
    })
}

/// Appends one validated pubkey line to authorized_keys via the daemon.
///
/// # Inputs
///
/// * `pubkey_line` — a single OpenSSH authorized_keys line to append.
#[tauri::command]
pub async fn host_append_authorized_pubkey(
    pubkey_line: String,
) -> Result<crate::host_trust::HostTrustAppendResult, BackrCommandError> {
    let v = crate::ipc_client::send(
        "host_append_authorized_pubkey",
        serde_json::json!({ "pubkey_line": pubkey_line }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!(
            "failed to deserialize pubkey append result: {e}"
        ))
    })
}

/// Lists every parsed pubkey entry in authorized_keys for the host Settings trusted-keys list.
#[tauri::command]
pub async fn host_list_authorized_pubkeys(
) -> Result<Vec<crate::host_trust::AuthorizedPubkeyEntry>, BackrCommandError> {
    let v =
        crate::ipc_client::send("host_list_authorized_pubkeys", serde_json::json!({})).await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!("failed to deserialize authorized pubkey list: {e}"))
    })
}

/// Removes one pubkey line (identified by exact raw_line match) from authorized_keys via the daemon.
///
/// # Inputs
///
/// * `raw_line` — exact pubkey line string to remove from authorized_keys.
#[tauri::command]
pub async fn host_remove_authorized_pubkey(
    raw_line: String,
) -> Result<crate::host_trust::HostRemovePubkeyResult, BackrCommandError> {
    let v = crate::ipc_client::send(
        "host_remove_authorized_pubkey",
        serde_json::json!({ "raw_line": raw_line }),
    )
    .await?;
    serde_json::from_value(v).map_err(|e| {
        BackrCommandError::config(format!(
            "failed to deserialize pubkey remove result: {e}"
        ))
    })
}
