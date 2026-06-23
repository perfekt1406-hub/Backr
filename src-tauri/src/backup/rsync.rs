/*
 * Local `rsync` invocations for snapshot backups and restores.
 * Builds `-e` ssh wrapper strings with escaped paths and streams progress lines as events.
 */

use std::path::Path;
use std::process::Stdio;

use shell_escape::escape;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::backup::ssh::remote_path_join;
use crate::error::BackrError;
use crate::progress_sink::SharedProgress;

/// Event name forwarded to the webview for raw rsync `--info=progress2` lines.
pub const BACKUP_PROGRESS_EVENT: &str = "backup://progress";

/// Builds the `ssh` command embedded in `rsync --rsh` for backup (accepts new host keys).
///
/// # Inputs
///
/// * `ssh_key` — expanded private key path.
/// * `known_hosts` — Backr-specific `known_hosts` file path.
///
/// # Returns
///
/// A single shell-safe string suitable for `rsync -e '<returned>'`.
fn ssh_rsh_backup(ssh_key: &str, known_hosts: &Path, ssh_port: u16) -> String {
    let key = escape(std::borrow::Cow::Borrowed(ssh_key));
    let kh_owned = known_hosts.to_string_lossy().into_owned();
    let kh = escape(std::borrow::Cow::Borrowed(kh_owned.as_str()));
    format!(
        "ssh -p {ssh_port} -i {key} -o StrictHostKeyChecking=accept-new -o BatchMode=yes -o UserKnownHostsFile={kh}",
    )
}

/// Builds the `ssh` command embedded in `rsync --rsh` for restore (batch mode, known_hosts file).
///
/// # Inputs
///
/// Same as [`ssh_rsh_backup`], but omits `StrictHostKeyChecking=accept-new` per restore plan.
fn ssh_rsh_restore(ssh_key: &str, known_hosts: &Path, ssh_port: u16) -> String {
    let key = escape(std::borrow::Cow::Borrowed(ssh_key));
    let kh_owned = known_hosts.to_string_lossy().into_owned();
    let kh = escape(std::borrow::Cow::Borrowed(kh_owned.as_str()));
    format!("ssh -p {ssh_port} -i {key} -o BatchMode=yes -o UserKnownHostsFile={kh}",)
}

/// Runs a backup rsync from a local project directory into a fresh remote snapshot folder.
///
/// # Inputs
///
/// * `local_project_dir` — absolute path to `.../Projects/<project>`; a trailing slash is appended for rsync semantics.
/// * `link_dest_remote` — optional absolute remote path of the previous snapshot for hardlink deltas.
/// * `user` / `host` — SSH target.
/// * `remote_dest_folder` — absolute remote directory for the new snapshot (parent dirs created implicitly by rsync).
/// * `sink` — progress line consumer (webview events or test collector).
#[allow(clippy::too_many_arguments)]
pub async fn rsync_backup_snapshot(
    sink: SharedProgress,
    ssh_key: &str,
    known_hosts: &Path,
    local_project_dir: &Path,
    link_dest_remote: Option<&str>,
    user: &str,
    host: &str,
    ssh_port: u16,
    remote_dest_folder: &str,
) -> Result<(), BackrError> {
    let mut src = local_project_dir.display().to_string();
    if !src.ends_with('/') {
        src.push('/');
    }

    let remote_url = format!("{user}@{host}:{remote_dest_folder}/");
    let rsh = ssh_rsh_backup(ssh_key, known_hosts, ssh_port);

    let mut cmd = Command::new("rsync");
    cmd.arg("--archive");
    cmd.arg("--hard-links");
    cmd.arg("--delete");
    cmd.arg("--info=progress2");
    cmd.arg("--human-readable");
    // Skip regenerable build output, dependency dirs, tool caches, and OS cruft so
    // snapshots stay small. `--delete` is paired with `--delete-excluded` so that a
    // path which becomes excluded later (e.g. after this list grows) is also pruned
    // from the remote snapshot instead of lingering forever.
    for pattern in crate::backup::excludes::BACKUP_EXCLUDES {
        cmd.arg("--exclude").arg(pattern);
    }
    cmd.arg("--delete-excluded");
    cmd.arg("-e").arg(&rsh);
    if let Some(prev) = link_dest_remote {
        cmd.arg(format!("--link-dest={prev}"));
    }
    cmd.arg(&src);
    cmd.arg(&remote_url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    run_rsync_emit_lines(sink, cmd).await
}

/// Restores a remote snapshot into a newly created local folder using rsync over ssh.
///
/// # Inputs
///
/// * `remote_snapshot_url` — `user@host:/abs/path/to/snapshot/` form; trailing slash added if missing.
/// * `local_destination` — local folder to receive files (caller ensures uniqueness).
pub async fn rsync_restore_snapshot(
    sink: SharedProgress,
    ssh_key: &str,
    known_hosts: &Path,
    remote_snapshot_url: &str,
    ssh_port: u16,
    local_destination: &Path,
) -> Result<(), BackrError> {
    let rsh = ssh_rsh_restore(ssh_key, known_hosts, ssh_port);
    let mut dest = local_destination.display().to_string();
    if !dest.ends_with('/') {
        dest.push('/');
    }

    let mut cmd = Command::new("rsync");
    cmd.arg("--archive");
    cmd.arg("--info=progress2");
    cmd.arg("--human-readable");
    cmd.arg("-e").arg(&rsh);
    let mut remote_with_slash = remote_snapshot_url.to_string();
    if !remote_with_slash.ends_with('/') {
        remote_with_slash.push('/');
    }
    cmd.arg(&remote_with_slash);
    cmd.arg(&dest);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    run_rsync_emit_lines(sink, cmd).await
}

/// Composes the absolute remote path for a snapshot folder used as `--link-dest`.
///
/// # Inputs
///
/// * `backup_root` — configured remote backup root (`/backups`).
pub fn absolute_remote_snapshot_path(backup_root: &str, project: &str, snapshot: &str) -> String {
    remote_path_join(&remote_path_join(backup_root, project), snapshot)
}

/// Computes the absolute remote directory path for syncing a new snapshot into.
pub fn remote_snapshot_dest_folder(
    backup_root: &str,
    project: &str,
    snapshot_name: &str,
) -> String {
    absolute_remote_snapshot_path(backup_root, project, snapshot_name)
}

/// Spawns rsync, streams merged stdout/stderr line-by-line to the progress sink, then checks exit status.
async fn run_rsync_emit_lines(sink: SharedProgress, mut cmd: Command) -> Result<(), BackrError> {
    let mut child = cmd
        .spawn()
        .map_err(|e| BackrError::Msg(format!("failed to spawn rsync: {e}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BackrError::Msg("rsync missing stdout pipe".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BackrError::Msg("rsync missing stderr pipe".into()))?;

    let sink_out = sink.clone();
    let out_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            sink_out.backup_progress_line(line);
        }
    });

    let sink_err = sink.clone();
    let err_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            sink_err.backup_progress_line(format!("[stderr] {line}"));
        }
    });

    let status = child.wait().await.map_err(BackrError::Io)?;
    let _ = out_task.await;
    let _ = err_task.await;

    if !status.success() {
        return Err(BackrError::Remote(format!(
            "rsync exited with status {:?}",
            status.code()
        )));
    }
    Ok(())
}
