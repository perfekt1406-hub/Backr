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

use std::path::PathBuf;

use serde::Serialize;

use crate::error::BackrError;
use swap::{BinarySwap, ServiceControl};

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

/// Result of an [`apply_update`] attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ApplyOutcome {
    /// Binaries were swapped from `from` to `to` and the daemon is healthy.
    Updated { from: String, to: String },
    /// Already on the latest release; nothing was changed.
    UpToDate { version: String },
    /// Refused because a backup is in progress (R7); nothing was changed.
    BackupInProgress,
}

/// Downloads, verifies, and applies the latest release, rolling back on failure.
///
/// # Inputs
/// * `service` — controls the daemon around the swap (injected; real impl is daemon-side).
/// * `is_busy` — returns true when a backup is in progress; checked at entry and again
///   immediately before the swap to avoid interrupting a backup that started mid-download (R7).
/// * `progress` — sink for human-readable progress lines the IPC layer forwards.
///
/// # Returns
/// [`ApplyOutcome`] describing what happened. Blocking (network + I/O) — callers on
/// an async runtime must use spawn_blocking.
pub fn apply_update<S: ServiceControl>(
    service: &S,
    is_busy: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<ApplyOutcome, BackrError> {
    // R7: never interrupt an in-progress backup.
    if is_busy() {
        return Ok(ApplyOutcome::BackupInProgress);
    }

    let current = crate::version().to_string();
    let slug = release::repo_slug();
    let token = release::github_token();
    let token = token.as_deref();

    progress("Checking for the latest release…");
    let latest = release::fetch_latest_release(&slug, token)?;
    if !release::is_newer(&latest.tag, &current) {
        return Ok(ApplyOutcome::UpToDate { version: current });
    }

    let staging = staging_dir()?;
    // Cleanup runs whether the swap succeeds, fails, or is skipped.
    let result = stage_and_apply(&latest, token, &staging, service, is_busy, progress);
    let _ = std::fs::remove_dir_all(&staging);
    result.map(|applied| {
        if applied {
            ApplyOutcome::Updated { from: current, to: latest.tag }
        } else {
            ApplyOutcome::BackupInProgress
        }
    })
}

/// Downloads + verifies every asset into `staging`, then swaps. Returns `Ok(true)`
/// when the swap ran, `Ok(false)` when it was skipped because a backup started.
fn stage_and_apply<S: ServiceControl>(
    latest: &release::ReleaseInfo,
    token: Option<&str>,
    staging: &std::path::Path,
    service: &S,
    is_busy: &dyn Fn() -> bool,
    progress: &dyn Fn(&str),
) -> Result<bool, BackrError> {
    let checksum = latest
        .checksum_asset()
        .ok_or_else(|| BackrError::Update("release has no SHA256SUMS asset".into()))?;
    progress("Downloading checksums…");
    let sums_path = staging.join(release::CHECKSUM_ASSET);
    release::download_to_file(&checksum.url, token, &sums_path)?;
    let sums = release::parse_sha256sums(
        &std::fs::read_to_string(&sums_path).map_err(BackrError::Io)?,
    );

    let mut swaps: Vec<BinarySwap> = Vec::new();
    for bin in release::RELEASE_BINARIES {
        let asset = latest
            .binary_asset(bin)
            .ok_or_else(|| BackrError::Update(format!("release missing asset for {bin}")))?;
        progress(&format!("Downloading {bin}…"));
        let staged = staging.join(&asset.name);
        release::download_to_file(&asset.url, token, &staged)?;
        let expected = sums
            .get(&asset.name)
            .ok_or_else(|| BackrError::Update(format!("no checksum for {}", asset.name)))?;
        release::verify_sha256(&staged, expected)?;
        let target = swap::installed_target_path(bin)?;
        swaps.push(BinarySwap { target, staged });
    }

    // Re-check right before the destructive step: a backup may have started during
    // the downloads, and the swap restarts the daemon (R7).
    if is_busy() {
        return Ok(false);
    }

    progress("Applying update…");
    swap::apply_swap(&swaps, service)?;
    Ok(true)
}

/// Creates a unique staging directory for downloaded assets (runtime dir, else temp).
fn staging_dir() -> Result<PathBuf, BackrError> {
    let base = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = base.join(format!("backr-update-{}-{}", std::process::id(), nanos));
    std::fs::create_dir_all(&dir).map_err(BackrError::Io)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Controller that fails the test if any method is called — proves the guard
    /// short-circuits before touching the service.
    struct UnusedService;
    impl ServiceControl for UnusedService {
        fn stop(&self) -> Result<(), BackrError> {
            panic!("service must not be touched when a backup is in progress");
        }
        fn start(&self) -> Result<(), BackrError> {
            panic!("service must not be touched when a backup is in progress");
        }
        fn health_check(&self) -> Result<(), BackrError> {
            panic!("service must not be touched when a backup is in progress");
        }
    }

    /// Covers AE1: apply while a backup is in progress returns the busy result and
    /// performs no network call and no swap (the controller would panic if touched).
    #[test]
    fn apply_update_refuses_while_backup_in_progress() {
        let outcome = apply_update(&UnusedService, &|| true, &|_| {})
            .expect("busy guard returns Ok, not Err");
        assert_eq!(outcome, ApplyOutcome::BackupInProgress);
    }
}
