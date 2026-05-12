/*
 * Purpose: Read-only system metadata for the laptop dashboard (hostname, OS, kernel).
 * Role: Surfaces `/etc/os-release`, `hostname`, and `uname` without pulling heavyweight crates.
 */

use chrono::Local;
use serde::Serialize;
use std::fs;
use std::process::Command;

/// Snapshot of local machine facts rendered beside backup tooling.
#[derive(Debug, Serialize)]
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

/// Parses `PRETTY_NAME="..."` from `/etc/os-release` when present.
///
/// # Returns
///
/// Trimmed quoted distro description or `None` when the file or field is missing.
fn read_os_release_pretty() -> Option<String> {
    let data = fs::read_to_string("/etc/os-release").ok()?;
    for raw in data.lines() {
        let line = raw.trim();
        let Some(rest) = line.strip_prefix("PRETTY_NAME=") else {
            continue;
        };
        let v = rest.trim().trim_matches('"').trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

/// Best-effort short hostname via the `hostname` executable (POSIX / Windows).
///
/// External: `std::process::Command::output` runs `hostname` with inherited environment.
fn hostname_via_bin() -> Option<String> {
    let out = Command::new("hostname").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Kernel version token via `uname -r` on Unix-like hosts.
///
/// External: `std::process::Command::output` invokes `/usr/bin/env`'s `uname` child when installed.
fn kernel_via_uname() -> Option<String> {
    let out = Command::new("uname").arg("-r").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Collects hostname, distro label, kernel, arch, user, and a sample wall-clock instant for the UI.
///
/// External: `chrono::Local::now` captures local timezone offset in RFC3339 serialization.
#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    let os_pretty = read_os_release_pretty().unwrap_or_else(|| {
        format!(
            "{} ({})",
            std::env::consts::OS,
            std::env::consts::FAMILY
        )
    });

    SystemInfo {
        hostname: hostname_via_bin(),
        os_pretty,
        kernel_release: kernel_via_uname(),
        arch: std::env::consts::ARCH.to_string(),
        user: std::env::var("USER")
            .ok()
            .or_else(|| std::env::var("USERNAME").ok()),
        sampled_at_rfc3339: Local::now().to_rfc3339(),
    }
}
