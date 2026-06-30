/*
 * Self-update engine shared by the daemon, CLI, and auto-update trigger.
 *
 * `release` handles GitHub release lookup, download, and SHA-256 verification;
 * this module composes those into the high-level check surface the IPC layer and
 * CLI call. The download → verify → swap → rollback orchestration (U5/U6) builds
 * on these primitives. All network/hashing is in-process (ureq + sha2).
 *
 * Blocking I/O: callers on an async runtime must use tokio::task::spawn_blocking.
 */

pub mod release;
pub mod swap;

use serde::Serialize;

use crate::error::BackrError;

/// Current-vs-latest version summary surfaced to the update UIs and CLI.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateStatus {
    /// Version embedded in the running binaries (the shared workspace version).
    pub current_version: String,
    /// Latest release tag, when a lookup succeeded.
    pub latest_version: Option<String>,
    /// True when `latest_version` is a strictly newer semver than `current_version`.
    pub update_available: bool,
}

/// Checks whether a newer release exists for the configured repo.
///
/// # Returns
///
/// An [`UpdateStatus`] comparing the embedded version against the latest release.
///
/// Blocking (network I/O) — callers on an async runtime must use spawn_blocking.
pub fn check_for_update() -> Result<UpdateStatus, BackrError> {
    let current = crate::version().to_string();
    let slug = release::repo_slug();
    let token = release::github_token();
    let latest = release::fetch_latest_release(&slug, token.as_deref())?;
    let available = release::is_newer(&latest.tag, &current);
    Ok(UpdateStatus {
        current_version: current,
        latest_version: Some(latest.tag),
        update_available: available,
    })
}
