/*
 * update_worker.rs — the out-of-process self-update worker.
 *
 * `backr update` IS the worker backrd launches (via systemd-run --scope) so the
 * daemon can be stopped, replaced, and restarted without killing the updater
 * (KTD4). It drives backr_core::update::apply_update with:
 *   - a real ServiceControl that stops/starts backrd.service and health-checks it
 *     by polling an IPC ping, and
 *   - an is-busy probe that asks the (still-running) daemon for backup status (R7).
 *
 * The flow is blocking by design: it must keep working while the daemon is down,
 * so it uses the synchronous IPC client, not the async one. Self-update is
 * Linux-first; on other platforms the service-control calls return an error.
 */

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::json;

use backr_core::error::BackrError;
use backr_core::update::swap::ServiceControl;
use backr_core::update::{self, ApplyOutcome};

use crate::client;

/// Real service controller for the worker: systemd (Linux) + IPC-ping health check.
pub struct SystemdServiceControl;

impl ServiceControl for SystemdServiceControl {
    fn stop(&self) -> Result<(), BackrError> {
        stop_daemon()
    }

    fn start(&self) -> Result<(), BackrError> {
        start_daemon()
    }

    /// Polls an IPC `ping` until the restarted daemon answers, or times out (~15s).
    fn health_check(&self) -> Result<(), BackrError> {
        for _ in 0..30 {
            if client::send_command_blocking("ping", json!({}), Duration::from_secs(2)).is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Err(BackrError::Update(
            "daemon did not become healthy after restart".into(),
        ))
    }
}

/// Runs the update worker: `--check` reports availability; otherwise download,
/// verify, swap, and restart via the engine.
///
/// # Inputs
/// * `check_only` — only report whether an update is available.
/// * `from_daemon` — launched by backrd; suppresses interactive progress lines.
/// * `json` — emit machine-readable JSON instead of prose.
pub fn run_update(check_only: bool, from_daemon: bool, json_out: bool) -> Result<()> {
    if check_only {
        let status = update::check_for_update().map_err(|e| anyhow!("{e}"))?;
        if json_out {
            println!("{}", serde_json::to_string(&status)?);
        } else if status.update_available {
            println!(
                "Update available: {} -> {}",
                status.current_version,
                status.latest_version.as_deref().unwrap_or("?")
            );
        } else {
            println!("Up to date ({}).", status.current_version);
        }
        return Ok(());
    }

    let service = SystemdServiceControl;
    let outcome = update::apply_update(
        &service,
        &backup_in_progress,
        &|line: &str| {
            if !from_daemon {
                println!("{line}");
            }
        },
    )
    .map_err(|e| anyhow!("{e}"))?;

    if json_out {
        println!("{}", serde_json::to_string(&outcome)?);
    } else {
        match outcome {
            ApplyOutcome::Updated { from, to } => {
                println!("Updated {from} -> {to}. The backup daemon was restarted.")
            }
            ApplyOutcome::UpToDate { version } => println!("Already up to date ({version})."),
            ApplyOutcome::BackupInProgress => {
                println!("A backup is in progress — update skipped. Try again when it finishes.")
            }
        }
    }
    Ok(())
}

/// Asks the (still-running) daemon whether a backup is in progress (R7 guard).
/// Treats an unreachable daemon as "not busy" so the guard never blocks falsely.
fn backup_in_progress() -> bool {
    client::send_command_blocking("get_backup_status", json!({}), Duration::from_secs(5))
        .ok()
        .and_then(|v| v.get("in_progress").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

/// Stops the backrd service so its binary can be replaced (Linux: systemd).
#[cfg(target_os = "linux")]
fn stop_daemon() -> Result<(), BackrError> {
    run_service_cmd("systemctl", &["--user", "stop", "backrd.service"])
}

/// Starts the backrd service after the swap (Linux: systemd).
#[cfg(target_os = "linux")]
fn start_daemon() -> Result<(), BackrError> {
    run_service_cmd("systemctl", &["--user", "start", "backrd.service"])
}

#[cfg(not(target_os = "linux"))]
fn stop_daemon() -> Result<(), BackrError> {
    Err(BackrError::Update(
        "self-update is Linux-only for now (macOS support is deferred)".into(),
    ))
}

#[cfg(not(target_os = "linux"))]
fn start_daemon() -> Result<(), BackrError> {
    Err(BackrError::Update(
        "self-update is Linux-only for now (macOS support is deferred)".into(),
    ))
}

/// Runs a service-control command, mapping a non-zero exit to an `Update` error.
#[cfg(target_os = "linux")]
fn run_service_cmd(program: &str, args: &[&str]) -> Result<(), BackrError> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|e| BackrError::Update(format!("could not run {program}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(BackrError::Update(format!(
            "{program} {} exited with {status}",
            args.join(" ")
        )))
    }
}
