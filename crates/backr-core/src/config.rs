/*
 * Typed view of `~/.config/backr/config.toml` plus helpers to load/save/resolve application paths.
 * Normalizes user-facing path fields (tilde expansion) when reading from disk.
 */

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::BackrError;

/// SSH target and remote backup root as stored under `[remote]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteConfig {
    pub host: String,
    pub user: String,
    pub ssh_key: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub backup_path: String,
}

fn default_ssh_port() -> u16 {
    22
}

/// Local filesystem roots as stored under `[local]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalConfig {
    pub projects_path: String,
}

/// Automatic backup cadence as stored under `[schedule]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleConfig {
    pub interval_hours: u32,
}

/// Persisted backup metadata as stored under `[state]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateConfig {
    pub last_backup_at: Option<DateTime<Utc>>,
}

/// Full persisted configuration document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub remote: RemoteConfig,
    pub local: LocalConfig,
    pub schedule: ScheduleConfig,
    pub state: StateConfig,
}

/// Returns the platform-specific path to `config.toml` under the Backr config directory.
///
/// # Returns
///
/// Absolute path, typically `~/.config/backr/config.toml` on Unix.
pub fn config_path() -> Result<PathBuf, BackrError> {
    let base = dirs::config_dir()
        .ok_or_else(|| BackrError::Config("could not resolve user config directory".into()))?;
    Ok(base.join("backr").join("config.toml"))
}

/// Expands `~` and environment variables in a path-like string using the `shellexpand` semantics.
///
/// # Inputs
///
/// * `value` — user or configuration-provided path that may contain `~`.
///
/// # Returns
///
/// Canonical-ish absolute path string suitable for `Path` construction.
pub fn expand_path_str(value: &str) -> Result<String, BackrError> {
    shellexpand::full(value)
        .map(|c| c.into_owned())
        .map_err(|e| BackrError::Msg(format!("could not expand path: {e}")))
}

/// Returns the absolute path to the isolated SSH `known_hosts` file used for backups.
///
/// # Returns
///
/// Path under `~/.config/backr/known_hosts`, creating parent directories as needed.
pub fn known_hosts_path() -> Result<PathBuf, BackrError> {
    let base = dirs::config_dir()
        .ok_or_else(|| BackrError::Config("could not resolve user config directory".into()))?;
    let dir = base.join("backr");
    std::fs::create_dir_all(&dir).map_err(BackrError::Io)?;
    Ok(dir.join("known_hosts"))
}

/// Loads `config.toml` from disk if present.
///
/// # Returns
///
/// `Ok(None)` when the file is missing; `Ok(Some(cfg))` when parsed; `Err` on I/O or TOML errors.
pub fn load_config() -> Result<Option<Config>, BackrError> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(BackrError::Io)?;
    let mut cfg: Config = toml::from_str(&raw)?;
    cfg.remote.ssh_key = expand_path_str(&cfg.remote.ssh_key)?;
    cfg.local.projects_path = expand_path_str(&cfg.local.projects_path)?;
    Ok(Some(cfg))
}

/// Persists the full configuration structure to disk atomically (write temp + rename).
///
/// # Inputs
///
/// * `config` — complete configuration snapshot to write.
pub fn save_config(config: &Config) -> Result<(), BackrError> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(BackrError::Io)?;
    }
    let serialized = toml::to_string_pretty(config)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, serialized).map_err(BackrError::Io)?;
    std::fs::rename(&tmp, &path).map_err(BackrError::Io)?;
    Ok(())
}

/// Returns the path to the SSH control socket directory for ControlMaster multiplexing.
///
/// Uses `XDG_RUNTIME_DIR` if set (guaranteed writable, tmpfs-backed on most Linux systems);
/// otherwise falls back to `~/.config/backr/ssh-control`. Creates the directory if it does
/// not yet exist.
///
/// The socket filename for a given host+port should be formed by the caller as:
/// `<returned_dir>/backr-<host>-<port>.sock`
///
/// # Returns
///
/// `Ok(PathBuf)` — absolute path to the existing directory on success.
/// `Err(BackrError)` — if neither `XDG_RUNTIME_DIR` nor the home directory can be resolved,
/// or if `create_dir_all` fails.
pub fn ssh_control_dir() -> Result<PathBuf, BackrError> {
    // Prefer XDG_RUNTIME_DIR (tmpfs-backed, cleaned on logout, world-standard on Linux/systemd).
    let dir = if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime).join("backr-ssh-control")
    } else {
        // Fall back to the Backr config directory so we always have a stable location.
        let base = dirs::config_dir()
            .ok_or_else(|| BackrError::Config("could not resolve user config directory".into()))?;
        base.join("backr").join("ssh-control")
    };
    std::fs::create_dir_all(&dir).map_err(BackrError::Io)?;
    Ok(dir)
}

/// Ensures a directory exists locally, creating parents as required.
///
/// # Inputs
///
/// * `dir` — directory path on disk.
pub fn ensure_dir_exists(dir: &Path) -> Result<(), BackrError> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(BackrError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that `ssh_control_dir` returns a path anchored under either `XDG_RUNTIME_DIR`
    /// or the user home directory, depending on the environment.
    #[test]
    fn ssh_control_dir_returns_valid_path() {
        let dir = ssh_control_dir().expect("ssh_control_dir should succeed");
        // Must be absolute so the SSH `-o ControlPath=` option resolves correctly.
        assert!(dir.is_absolute(), "control dir must be absolute: {dir:?}");
        // The directory must exist after the call.
        assert!(dir.exists(), "control dir must exist after creation: {dir:?}");

        // Path must be rooted under XDG_RUNTIME_DIR or the user home/config dir.
        let under_xdg = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .map(|r| dir.starts_with(r))
            .unwrap_or(false);
        let under_home = dirs::home_dir()
            .map(|h| dir.starts_with(h))
            .unwrap_or(false);
        assert!(
            under_xdg || under_home,
            "control dir {dir:?} must be under XDG_RUNTIME_DIR or home"
        );
    }

    /// Verifies that calling `ssh_control_dir` twice returns the same path (deterministic).
    #[test]
    fn ssh_control_dir_is_deterministic() {
        let first = ssh_control_dir().expect("first call should succeed");
        let second = ssh_control_dir().expect("second call should succeed");
        assert_eq!(first, second, "ssh_control_dir must return the same path on repeated calls");
    }
}
