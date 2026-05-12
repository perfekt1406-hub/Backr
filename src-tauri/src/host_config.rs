/*
 * Purpose: Reads `/etc/backr/host.toml` marker written by setup-backup-host.sh.
 * Role: Enables host-dashboard mode on backup servers without a Backr client config file.
 */

use std::path::Path;

use serde::Deserialize;

/// Path where `setup-backup-host.sh` drops machine-readable backup-root metadata.
pub const HOST_MARKER_PATH: &str = "/etc/backr/host.toml";

/// Parsed host marker — minimal fields for dashboard reads (SSH user is informational only).
#[derive(Debug, Clone, Deserialize)]
pub struct HostMarkerFile {
    pub backup_root: String,
    pub ssh_user: Option<String>,
}

/// Loads `/etc/backr/host.toml` when present and readable.
///
/// # Returns
///
/// `Ok(None)` when missing; `Ok(Some)` when parsed; `Err` on unreadable or invalid TOML.
///
/// External: `toml::from_str` deserializes TOML text into [`HostMarkerFile`].
pub fn load_host_marker() -> Result<Option<HostMarkerFile>, String> {
    let path = Path::new(HOST_MARKER_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", HOST_MARKER_PATH))?;
    let marker: HostMarkerFile =
        toml::from_str(&raw).map_err(|e| format!("parse {}: {e}", HOST_MARKER_PATH))?;
    Ok(Some(marker))
}
