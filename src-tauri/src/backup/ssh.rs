/*
 * Remote helpers that invoke local `ssh` to list snapshots/files and verify connectivity.
 * OpenSSH joins the remote argv with spaces and runs the result through the remote login
 * shell (`sh -c "…"`), so each remote token is single-quote escaped via `join_remote_command`
 * before being sent — otherwise a path with spaces or shell metacharacters (e.g. a project
 * folder named `Submissions (Copy)`) is mis-parsed remotely.
 * `find -printf` record output uses ASCII RS/US delimiters: newlines/tabs must not appear in argv
 * tokens because the remote login shell truncates them.
 */

use std::borrow::Cow;
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;
use shell_escape::escape;
use tokio::process::Command;

use crate::error::BackrError;

/// Joins remote argv tokens into a single shell-safe command string for OpenSSH.
///
/// OpenSSH concatenates the remote arguments with spaces and runs the result through the
/// remote login shell, so any token containing spaces, parentheses, or other shell
/// metacharacters would be mis-parsed remotely. Each token is single-quote escaped so the
/// remote shell reconstructs the exact intended argv.
///
/// # Inputs
///
/// * `remote_argv` — command followed by its arguments to run on the remote host.
///
/// # Returns
///
/// A single shell-escaped command line to pass as one `ssh` argument.
fn join_remote_command(remote_argv: &[String]) -> String {
    remote_argv
        .iter()
        .map(|a| escape(Cow::Borrowed(a.as_str())).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Snapshot directory names must match this pattern (YYYY-MM-DD_HH-MM-SS).
static SNAPSHOT_NAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}$").expect("valid snapshot regex")
});

/// Returns true when `name` matches the snapshot directory naming convention.
///
/// # Inputs
///
/// * `name` — path component (no slashes).
pub fn is_valid_snapshot_name(name: &str) -> bool {
    SNAPSHOT_NAME_RE.is_match(name)
}

/// Joins two remote Unix path segments without duplicating slashes.
///
/// # Inputs
///
/// * `base` — absolute remote directory (e.g. `/backups/my-app`).
/// * `child` — relative subdirectory or empty for `base` itself.
///
/// # Returns
///
/// A normalized remote path using `/` separators.
pub fn remote_path_join(base: &str, child: &str) -> String {
    let base = base.trim_end_matches('/');
    if child.is_empty() || child == "." {
        base.to_string()
    } else {
        format!("{}/{}", base, child.trim_start_matches('/'))
    }
}

/// Builds the absolute remote directory backing up a single project (contains snapshot folders).
///
/// # Inputs
///
/// * `backup_root` — configured `[remote].backup_path`.
/// * `project` — directory name under the local projects root.
///
/// # Returns
///
/// Remote path string such as `/backups/my-app`.
pub fn remote_project_dir(backup_root: &str, project: &str) -> String {
    remote_path_join(backup_root, project)
}

/// Ensures the remote per-project directory exists (`mkdir -p`) so rsync can create snapshot subfolders.
///
/// # Inputs
///
/// * `dir` — absolute path, typically from [`remote_project_dir`].
///
/// # Returns
///
/// `Ok(())` when `ssh` reports success; otherwise a [`BackrError::Remote`] with stderr context.
pub async fn ensure_remote_dir_exists(
    ssh_key: &str,
    known_hosts: &Path,
    host: &str,
    user: &str,
    ssh_port: u16,
    dir: &str,
) -> Result<(), BackrError> {
    let argv = vec!["mkdir".to_string(), "-p".to_string(), dir.to_string()];
    ssh_exec_trimmed(ssh_key, known_hosts, host, user, ssh_port, true, &argv).await?;
    Ok(())
}

/// Shared success path for `ssh` runs — preserves raw stdout bytes without trimming.
///
/// External: `tokio::process::Command::output` captures merged streams from the local `ssh` child.
async fn command_output(mut cmd: Command) -> Result<std::process::Output, BackrError> {
    let out = cmd.output().await.map_err(BackrError::Io)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(BackrError::Remote(format!(
            "ssh remote command failed (status {:?}): {}",
            out.status.code(),
            stderr.trim()
        )));
    }
    Ok(out)
}

/// Executes `ssh` with shared options and returns stdout as a trimmed string on success (status 0).
///
/// # Inputs
///
/// * `ssh_key` — expanded local path to the private key.
/// * `known_hosts` — local `known_hosts` path used to isolate Backr from the user SSH defaults.
/// * `host` / `user` — remote target.
/// * `ssh_port` — SSH server port (`22` typical; QEMU `hostfwd` often uses something like `2222`).
/// * `accept_new` — when true, enables `StrictHostKeyChecking=accept-new` (backup runs); restore uses stricter defaults per plan.
/// * `remote_argv` — argv for the remote process (`find`, paths, etc.).
///
/// # Returns
///
/// Trimmed UTF-8 stdout, or an error describing stderr or non-zero exit.
pub async fn ssh_exec_trimmed(
    ssh_key: &str,
    known_hosts: &Path,
    host: &str,
    user: &str,
    ssh_port: u16,
    accept_new: bool,
    remote_argv: &[String],
) -> Result<String, BackrError> {
    let mut cmd = ssh_base_command(ssh_key, known_hosts, accept_new, ssh_port);
    cmd.arg(format!("{user}@{host}"));
    cmd.arg(join_remote_command(remote_argv));
    run_and_capture(cmd).await
}

/// Reads up to `max_bytes` from a regular file under a snapshot via remote `head -c`.
///
/// # Inputs
///
/// * `relative_file_path` — path relative to the snapshot root (`src/main.rs`), no `..` segments.
///
/// # Returns
///
/// Raw bytes as captured from SSH stdout (not newline-trimmed).
///
/// External: remote `head -c` truncates larger files without error.
#[allow(clippy::too_many_arguments)]
pub async fn remote_read_file_bytes(
    ssh_key: &str,
    known_hosts: &Path,
    host: &str,
    user: &str,
    ssh_port: u16,
    backup_root: &str,
    project: &str,
    snapshot: &str,
    relative_file_path: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, BackrError> {
    let rel = relative_file_path.trim().trim_start_matches('/');
    if rel.is_empty() {
        return Err(BackrError::Msg("empty file path".into()));
    }
    if rel.contains("..") || rel.contains('\0') || rel.contains('\n') {
        return Err(BackrError::Msg("invalid relative path".into()));
    }
    let snapshot_root = remote_path_join(
        &remote_path_join(&remote_project_dir(backup_root, project), snapshot),
        "",
    );
    let abs = remote_path_join(&snapshot_root, rel);
    let argv = vec![
        "head".to_string(),
        "-c".to_string(),
        max_bytes.to_string(),
        abs,
    ];
    let mut cmd = ssh_base_command(ssh_key, known_hosts, true, ssh_port);
    cmd.arg(format!("{user}@{host}"));
    cmd.arg(join_remote_command(&argv));
    run_and_capture_stdout_exact(cmd).await
}

/// Runs a lightweight handshake command to validate credentials reachability.
///
/// # Inputs
///
/// * `host`, `user`, `ssh_key`, `ssh_port` — connection parameters supplied by the setup wizard.
pub async fn test_connection(
    host: &str,
    user: &str,
    ssh_key: &str,
    ssh_port: u16,
) -> Result<(), BackrError> {
    let known = crate::config::known_hosts_path()?;
    let mut cmd = ssh_base_command(ssh_key, &known, true, ssh_port);
    cmd.arg(format!("{user}@{host}"));
    cmd.arg("echo").arg("backr_ok");
    let out = run_and_capture(cmd).await?;
    if out.trim() == "backr_ok" {
        Ok(())
    } else {
        Err(BackrError::Remote(format!(
            "unexpected handshake output: {out}"
        )))
    }
}

/// Lists snapshot folder names for a project on the remote host, newest-first.
///
/// # Inputs
///
/// * `backup_root`, `project` — determine `/backups/<project>` remotely.
pub async fn remote_list_snapshot_names(
    ssh_key: &str,
    known_hosts: &Path,
    host: &str,
    user: &str,
    ssh_port: u16,
    backup_root: &str,
    project: &str,
) -> Result<Vec<String>, BackrError> {
    let dir = remote_project_dir(backup_root, project);
    let argv = vec![
        "find".to_string(),
        dir,
        "-mindepth".to_string(),
        "1".to_string(),
        "-maxdepth".to_string(),
        "1".to_string(),
        "-type".to_string(),
        "d".to_string(),
        "-printf".to_string(),
        // RS (0x1e): `find` newlines must not appear in any single argv token — OpenSSH runs the remote
        // command via the user's login shell (`sh -c "…joined args…"`), which truncates embedded `\n`.
        "%f\u{1e}".to_string(),
    ];
    let raw = ssh_exec_trimmed(ssh_key, known_hosts, host, user, ssh_port, true, &argv).await?;
    let mut names: Vec<String> = raw
        .split('\u{1e}')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| is_valid_snapshot_name(s))
        .map(|s| s.to_string())
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    Ok(names)
}

/// One directory entry inside a snapshot, parsed from `find -printf` output.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteFindEntry {
    pub file_type: char,
    pub size: u64,
    pub mtime_unix: f64,
    pub name: String,
}

/// Lists immediate children of a snapshot path using `find -maxdepth 1` with stable `-printf`.
///
/// # Inputs
///
/// * `remote_subpath` — optional relative path inside the snapshot; empty requests the snapshot root.
#[allow(clippy::too_many_arguments)]
pub async fn remote_list_children(
    ssh_key: &str,
    known_hosts: &Path,
    host: &str,
    user: &str,
    ssh_port: u16,
    backup_root: &str,
    project: &str,
    snapshot: &str,
    remote_subpath: &str,
) -> Result<Vec<RemoteFindEntry>, BackrError> {
    let snapshot_root = remote_path_join(
        &remote_path_join(&remote_project_dir(backup_root, project), snapshot),
        "",
    );
    let scan_base = if remote_subpath.is_empty() || remote_subpath == "." {
        snapshot_root
    } else {
        remote_path_join(&snapshot_root, remote_subpath)
    };
    let argv = vec![
        "find".to_string(),
        scan_base,
        "-mindepth".to_string(),
        "1".to_string(),
        "-maxdepth".to_string(),
        "1".to_string(),
        "-printf".to_string(),
        // US (0x1f) between columns, RS (0x1e) after each row — see snapshot list for why not `\n`/`\t`.
        "%y\u{1f}%s\u{1f}%T@\u{1f}%f\u{1e}".to_string(),
    ];
    let raw = ssh_exec_trimmed(ssh_key, known_hosts, host, user, ssh_port, true, &argv).await?;
    let mut rows = Vec::new();
    for record in raw.split('\u{1e}') {
        let record = record.trim_end();
        if record.is_empty() {
            continue;
        }
        let mut cols = record.splitn(4, '\u{1f}');
        let ty = cols
            .next()
            .ok_or_else(|| BackrError::Remote("malformed find row".into()))?;
        let size = cols
            .next()
            .ok_or_else(|| BackrError::Remote("malformed find row".into()))?;
        let mtime = cols
            .next()
            .ok_or_else(|| BackrError::Remote("malformed find row".into()))?;
        let name = cols
            .next()
            .ok_or_else(|| BackrError::Remote("malformed find row".into()))?;
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let file_type = ty
            .chars()
            .next()
            .ok_or_else(|| BackrError::Remote("missing file type".into()))?;
        let size_u = size
            .parse::<u64>()
            .map_err(|_| BackrError::Remote("invalid size in find output".into()))?;
        let mtime_f = mtime
            .parse::<f64>()
            .map_err(|_| BackrError::Remote("invalid mtime in find output".into()))?;
        rows.push(RemoteFindEntry {
            file_type,
            size: size_u,
            mtime_unix: mtime_f,
            name: name.to_string(),
        });
    }
    Ok(rows)
}

/// Builds the base `ssh` invocation shared by backup and inspection helpers.
///
/// # Inputs
///
/// * `accept_new` — enables accepting new host keys during backups.
/// * `ssh_port` — target TCP port for `ssh`.
fn ssh_base_command(ssh_key: &str, known_hosts: &Path, accept_new: bool, ssh_port: u16) -> Command {
    let mut c = Command::new("ssh");
    c.arg("-p").arg(ssh_port.to_string());
    c.arg("-i").arg(ssh_key);
    c.arg("-o").arg("ConnectTimeout=15");
    c.arg("-o").arg("BatchMode=yes");
    if accept_new {
        c.arg("-o").arg("StrictHostKeyChecking=accept-new");
    } else {
        c.arg("-o").arg("StrictHostKeyChecking=yes");
    }
    c.arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts.display()));
    c
}

/// Runs `tokio::process::Command`, captures stdout/stderr, maps non-zero exits to errors.
async fn run_and_capture(cmd: Command) -> Result<String, BackrError> {
    let out = command_output(cmd).await?;
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

/// Captures remote stdout bytes verbatim — used for file reads where trimming corrupts payload.
///
/// External: same SSH invocation shape as [`run_and_capture`].
async fn run_and_capture_stdout_exact(cmd: Command) -> Result<Vec<u8>, BackrError> {
    let out = command_output(cmd).await?;
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures canonical snapshot basename detection matches dashboard regex expectations.
    #[test]
    fn valid_snapshot_names() {
        assert!(is_valid_snapshot_name("2026-05-11_09-30-45"));
        assert!(!is_valid_snapshot_name("not-a-snapshot"));
        assert!(!is_valid_snapshot_name("2026_05_11_09-30-45"));
        assert!(!is_valid_snapshot_name(""));
    }

    /// Confirms POSIX remote joins normalize slashes deterministically for rsync endpoints.
    #[test]
    fn remote_path_join_trims_duplicate_slashes() {
        assert_eq!(remote_path_join("/backups", "proj"), "/backups/proj");
        assert_eq!(remote_path_join("/backups/", "proj"), "/backups/proj");
        assert_eq!(remote_path_join("/backups", ""), "/backups");
        assert_eq!(remote_path_join("/backups", "/nested"), "/backups/nested");
    }

    #[test]
    fn remote_project_dir_is_backup_plus_child() {
        assert_eq!(
            remote_project_dir("/srv/backups", "my-app"),
            "/srv/backups/my-app"
        );
    }
}
