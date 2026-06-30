/*
 * Binary swap + rollback for the self-update flow (KTD6).
 *
 * Replaces the three installed binaries in lockstep, restarting the daemon, and
 * rolling back on any failure so a bad update never leaves a client unable to
 * back up. The algorithm is parameterized over a `ServiceControl` trait and
 * explicit target/staged paths, so it is fully unit-testable against temp dirs
 * with a fake controller — it never assumes (or touches) the real service or the
 * real install. The concrete systemd/launchd + IPC-ping controller lives in the
 * daemon (it needs IPC context) and runs only when an update is actually applied.
 *
 * Sequence: pre-flight → stop service → swap each (old kept as `.bak`) → start →
 * health check → discard `.bak` on success, or restore `.bak` + restart on failure.
 */

use std::path::{Path, PathBuf};

use crate::error::BackrError;

/// Controls the backrd service around a swap.
///
/// Abstracted so the swap algorithm is testable without touching the real
/// service. `health_check` returning `Err` after restart triggers a rollback.
pub trait ServiceControl {
    /// Stops the running daemon so its executable can be replaced.
    fn stop(&self) -> Result<(), BackrError>;
    /// Starts the daemon after the binaries have been swapped (or restored).
    fn start(&self) -> Result<(), BackrError>;
    /// Returns `Ok(())` when the restarted daemon is healthy; `Err` triggers rollback.
    fn health_check(&self) -> Result<(), BackrError>;
}

/// One binary to replace: the installed target and its verified staged replacement.
#[derive(Debug, Clone)]
pub struct BinarySwap {
    /// Installed path to overwrite (e.g. `~/.local/bin/backrd`).
    pub target: PathBuf,
    /// Verified downloaded replacement (checksum already confirmed — see `release`).
    pub staged: PathBuf,
}

/// Resolves the installed path of a release binary on this machine.
///
/// `backrd` and `backr` live under `~/.local/bin`; `backr-app` is the real file
/// under `~/.local/share/backr` (the `~/.local/bin/backr-app` entry is a symlink
/// to it), so it must be replaced at the real path, not through the symlink.
pub fn installed_target_path(bin: &str) -> Result<PathBuf, BackrError> {
    let home = dirs::home_dir()
        .ok_or_else(|| BackrError::Update("could not resolve home directory".into()))?;
    let path = match bin {
        "backr-app" => home.join(".local/share/backr/backr-app"),
        other => home.join(".local/bin").join(other),
    };
    Ok(path)
}

/// Returns the `.bak` sibling path used to preserve the prior binary during a swap.
fn bak_path(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".bak");
    target.with_file_name(name)
}

/// Replaces all binaries in lockstep, restarting the service, rolling back on failure.
///
/// # Inputs
/// * `swaps` — target/staged pairs (staged files must already be checksum-verified).
/// * `service` — controller used to stop, start, and health-check the daemon.
///
/// # Returns
/// `Ok(())` when every binary is replaced and the restarted daemon is healthy.
/// On any failure after the service is stopped, completed swaps are restored from
/// their `.bak` copies, the prior daemon is restarted, and an `Update` error is
/// returned — leaving a working previous version (KTD6).
pub fn apply_swap<S: ServiceControl>(swaps: &[BinarySwap], service: &S) -> Result<(), BackrError> {
    // Pre-flight before touching anything: every staged file must exist and every
    // target directory must be present, so we never stop the service for a doomed swap.
    for s in swaps {
        if !s.staged.is_file() {
            return Err(BackrError::Update(format!(
                "staged binary missing: {}",
                s.staged.display()
            )));
        }
        let dir = s.target.parent().ok_or_else(|| {
            BackrError::Update(format!("target has no parent directory: {}", s.target.display()))
        })?;
        if !dir.is_dir() {
            return Err(BackrError::Update(format!(
                "target directory missing: {}",
                dir.display()
            )));
        }
    }

    service.stop()?;

    // Track completed swaps so we can roll them back (in reverse) on failure.
    let mut done: Vec<&BinarySwap> = Vec::new();
    let mut failure: Option<BackrError> = None;

    for s in swaps {
        match swap_one(s) {
            Ok(()) => done.push(s),
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }

    // If every binary swapped, bring the service back and verify health.
    if failure.is_none() {
        match service.start().and_then(|()| service.health_check()) {
            Ok(()) => {
                // Success: discard the `.bak` copies (best-effort).
                for s in swaps {
                    let _ = std::fs::remove_file(bak_path(&s.target));
                }
                return Ok(());
            }
            Err(e) => failure = Some(e),
        }
    }

    // Failure path: restore completed swaps, restart the prior service, surface the error.
    let err = failure.unwrap_or_else(|| BackrError::Update("swap failed".into()));
    for s in done.iter().rev() {
        let _ = rollback_one(s);
    }
    let _ = service.start();
    Err(BackrError::Update(format!(
        "update failed and was rolled back: {err}"
    )))
}

/// Replaces a single target with its staged file, preserving the old one as `.bak`.
fn swap_one(s: &BinarySwap) -> Result<(), BackrError> {
    let bak = bak_path(&s.target);
    // Preserve the current binary (if present) so we can roll back.
    if s.target.exists() {
        std::fs::rename(&s.target, &bak).map_err(|e| {
            BackrError::Update(format!("could not back up {}: {e}", s.target.display()))
        })?;
    }
    // Install the replacement; on failure, restore the just-moved original.
    if let Err(e) = install_executable(&s.staged, &s.target) {
        if bak.exists() {
            let _ = std::fs::rename(&bak, &s.target);
        }
        return Err(e);
    }
    Ok(())
}

/// Restores a single target from its `.bak` after a failed update.
fn rollback_one(s: &BinarySwap) -> Result<(), BackrError> {
    let bak = bak_path(&s.target);
    if bak.exists() {
        let _ = std::fs::remove_file(&s.target);
        std::fs::rename(&bak, &s.target).map_err(|e| {
            BackrError::Update(format!("rollback failed for {}: {e}", s.target.display()))
        })?;
    }
    Ok(())
}

/// Copies `staged` into `target` with executable permissions, atomically.
///
/// Stages into a temp file in the target's own directory (same filesystem) then
/// renames over the target so the binary is never half-written in place.
fn install_executable(staged: &Path, target: &Path) -> Result<(), BackrError> {
    let dir = target
        .parent()
        .ok_or_else(|| BackrError::Update(format!("target has no parent: {}", target.display())))?;
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("backr-bin");
    let tmp = dir.join(format!(".{file_name}.new"));
    std::fs::copy(staged, &tmp)
        .map_err(|e| BackrError::Update(format!("could not stage {}: {e}", target.display())))?;
    set_executable(&tmp)?;
    std::fs::rename(&tmp, target)
        .map_err(|e| BackrError::Update(format!("could not install {}: {e}", target.display())))?;
    Ok(())
}

/// Marks a file executable (0o755) on Unix; a no-op elsewhere.
#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), BackrError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).map_err(BackrError::Io)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(BackrError::Io)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<(), BackrError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A test controller that records calls and can be made to fail its health check.
    /// Touches nothing real — no systemd, no install.
    struct FakeService {
        stopped: Cell<bool>,
        starts: Cell<u32>,
        healthy: bool,
    }

    impl FakeService {
        fn new(healthy: bool) -> Self {
            Self {
                stopped: Cell::new(false),
                starts: Cell::new(0),
                healthy,
            }
        }
    }

    impl ServiceControl for FakeService {
        fn stop(&self) -> Result<(), BackrError> {
            self.stopped.set(true);
            Ok(())
        }
        fn start(&self) -> Result<(), BackrError> {
            self.starts.set(self.starts.get() + 1);
            Ok(())
        }
        fn health_check(&self) -> Result<(), BackrError> {
            if self.healthy {
                Ok(())
            } else {
                Err(BackrError::Update("daemon unhealthy after restart".into()))
            }
        }
    }

    /// Creates a unique temp directory for a test (never under the real install).
    fn temp_root(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("backr-swap-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Builds a bin dir with old binaries and a staging dir with new ones.
    fn fixture(base: &Path, names: &[&str]) -> Vec<BinarySwap> {
        let bindir = base.join("bin");
        let staging = base.join("staging");
        std::fs::create_dir_all(&bindir).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        names
            .iter()
            .map(|name| {
                let target = bindir.join(name);
                std::fs::write(&target, format!("OLD {name}")).unwrap();
                let staged = staging.join(name);
                std::fs::write(&staged, format!("NEW {name}")).unwrap();
                BinarySwap { target, staged }
            })
            .collect()
    }

    #[test]
    fn successful_swap_replaces_binaries_and_clears_backups() {
        let base = temp_root("ok");
        let names = ["backrd", "backr-app", "backr"];
        let swaps = fixture(&base, &names);
        let svc = FakeService::new(true);

        apply_swap(&swaps, &svc).expect("swap should succeed");

        assert!(svc.stopped.get(), "service must be stopped for the swap");
        assert_eq!(svc.starts.get(), 1, "service started exactly once on success");
        for (name, s) in names.iter().zip(&swaps) {
            assert_eq!(std::fs::read_to_string(&s.target).unwrap(), format!("NEW {name}"));
            assert!(!bak_path(&s.target).exists(), "no .bak residue after success");
        }
        std::fs::remove_dir_all(&base).ok();
    }

    /// Covers AE2: an unhealthy daemon after restart rolls every binary back.
    #[test]
    fn unhealthy_restart_rolls_back_all_binaries() {
        let base = temp_root("rollback");
        let names = ["backrd", "backr-app", "backr"];
        let swaps = fixture(&base, &names);
        let svc = FakeService::new(false); // health check fails

        let err = apply_swap(&swaps, &svc).unwrap_err();
        assert!(matches!(err, BackrError::Update(_)));

        for (name, s) in names.iter().zip(&swaps) {
            assert_eq!(
                std::fs::read_to_string(&s.target).unwrap(),
                format!("OLD {name}"),
                "binary restored to the previous version"
            );
            assert!(!bak_path(&s.target).exists(), ".bak consumed by rollback");
        }
        assert!(svc.stopped.get());
        assert_eq!(svc.starts.get(), 2, "started once for health check, once after rollback");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn missing_staged_fails_preflight_without_stopping_service() {
        let base = temp_root("preflight");
        let bindir = base.join("bin");
        std::fs::create_dir_all(&bindir).unwrap();
        let target = bindir.join("backrd");
        std::fs::write(&target, "OLD backrd").unwrap();
        // Staged file intentionally not created.
        let staged = base.join("staging").join("backrd");
        let swaps = vec![BinarySwap { target: target.clone(), staged }];
        let svc = FakeService::new(true);

        let err = apply_swap(&swaps, &svc).unwrap_err();
        assert!(matches!(err, BackrError::Update(_)));
        assert!(!svc.stopped.get(), "must not stop the service when pre-flight fails");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "OLD backrd",
            "target untouched on pre-flight failure"
        );
        std::fs::remove_dir_all(&base).ok();
    }
}
