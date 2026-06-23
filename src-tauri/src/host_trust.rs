/*
 * Purpose: Backup-host «Trust keys» flow — inspect authorized_keys and append laptop pubkey lines from the UI.
 * Role: Used only when Backr boots in host-dashboard mode (`/etc/backr/host.toml`). Writes are best-effort; falls back to a sudo shell snippet when the desktop user cannot write `~backup/.ssh/authorized_keys`.
 */

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::host_config::read_host_dashboard_marker;

static PUBKEY_PREFIX_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(ssh-rsa|ssh-ed25519|ssh-dss|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521|sk-ssh-ed25519|sk-ecdsa-sha2-nistp256)\s")
        .expect("pubkey prefix regex")
});

/// Wraps `s` in safe bash single quotes for embedding in generated sudo snippets.
///
/// # Inputs
///
/// * `s` — raw user-controlled pubkey line.
///
/// # Outputs
///
/// Bash-escaped token such as `'ssh-ed25519 AAA…'`.
pub(crate) fn bash_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Trims Windows CRLF and keeps the first non-empty line (authorized_keys is one line per key).
///
/// # Outputs
///
/// Normalized single-line pubkey string or error when blank.
pub fn normalize_pubkey_line(raw: &str) -> Result<String, String> {
    let t = raw.trim().replace('\r', "");
    let line = t.lines().next().unwrap_or("").trim().to_string();
    if line.is_empty() {
        return Err("empty pubkey line".into());
    }
    Ok(line)
}

/// Returns Ok when `line` looks like an OpenSSH authorized_keys entry.
///
/// # Outputs
///
/// Err with reason when validation fails.
pub fn validate_pubkey_line(line: &str) -> Result<(), String> {
    if line.starts_with('#') {
        return Err("comments are not valid pubkey lines".into());
    }
    if !PUBKEY_PREFIX_RE.is_match(line) {
        return Err("not a recognized OpenSSH public key line".into());
    }
    Ok(())
}

/// Counts plausible pubkey lines in `authorized_keys` text (comments/blanks skipped).
///
/// # Inputs
///
/// * `content` — full file body.
pub fn count_pubkey_lines(content: &str) -> usize {
    content
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && validate_pubkey_line(t).is_ok()
        })
        .count()
}

fn resolve_ssh_user() -> String {
    read_host_dashboard_marker()
        .and_then(|m| m.ssh_user)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "backr".to_string())
}

/// Resolves passwd home directory via `getent passwd LOGIN`.
///
/// # Outputs
///
/// Absolute home path or descriptive error.
pub fn passwd_home(login: &str) -> Result<PathBuf, String> {
    let out = Command::new("getent")
        .args(["passwd", login])
        .output()
        .map_err(|e| format!("getent passwd failed: {e}"))?;
    if !out.status.success() {
        return Err(format!("no passwd entry for user {login}"));
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let fields: Vec<&str> = line.trim().split(':').collect();
    if fields.len() < 7 {
        return Err("malformed getent passwd line".into());
    }
    Ok(PathBuf::from(fields[5]))
}

fn authorized_keys_path() -> Result<(String, PathBuf), String> {
    let ssh_user = resolve_ssh_user();
    let home = passwd_home(&ssh_user)?;
    let ak = home.join(".ssh").join("authorized_keys");
    Ok((ssh_user, ak))
}

/// Builds a short sudo snippet the operator can paste when the GUI user cannot write `authorized_keys`.
///
/// # Inputs
///
/// * `ssh_user` — backup UNIX account (usually **backr**).
/// * `line` — validated pubkey line.
/// * `ak` — absolute authorized_keys path.
fn build_sudo_append_snippet(ssh_user: &str, line: &str, ak: &Path) -> String {
    let ak_s = ak.to_string_lossy();
    let ak_q = bash_single_quote(&ak_s);
    let line_q = bash_single_quote(line);
    let dir = ak.parent().unwrap_or_else(|| Path::new("/"));
    let dir_q = bash_single_quote(&dir.to_string_lossy());
    format!(
        "# Run on this backup machine (terminal):\n\
sudo install -d -m 700 -o {user} -g {user} {dir_q}\n\
printf '%s\\n' {line_q} | sudo tee -a {ak_q} >/dev/null\n\
sudo chown {user}:{user} {ak_q}\n\
sudo chmod 600 {ak_q}\n",
        user = ssh_user,
        dir_q = dir_q,
        line_q = line_q,
        ak_q = ak_q,
    )
}

/// JSON row for [`super::host_cmd::host_trust_status`].
#[derive(Debug, Serialize)]
pub struct HostTrustStatus {
    pub ssh_user: String,
    pub authorized_keys_path: String,
    pub pubkey_line_count: usize,
}

/// Result row for [`super::host_cmd::host_append_authorized_pubkey`].
#[derive(Debug, Serialize)]
pub struct HostTrustAppendResult {
    pub appended: bool,
    pub skipped_duplicate: bool,
    pub pubkey_line_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sudo_script: Option<String>,
    pub message: String,
}

/// Reads current authorized_keys stats for the Trust UI.
///
/// # Outputs
///
/// [`HostTrustStatus`] or error when host marker / passwd lookup fails.
pub fn host_trust_status_impl() -> Result<HostTrustStatus, String> {
    let (ssh_user, ak_path) = authorized_keys_path()?;
    let path_str = ak_path.to_string_lossy().to_string();
    let content = if ak_path.is_file() {
        std::fs::read_to_string(&ak_path).unwrap_or_default()
    } else {
        String::new()
    };
    Ok(HostTrustStatus {
        ssh_user,
        authorized_keys_path: path_str,
        pubkey_line_count: count_pubkey_lines(&content),
    })
}

/// Appends `pubkey_line` when writable; otherwise returns a sudo shell snippet without failing the invoke.
///
/// # Inputs
///
/// * `pubkey_line` — single OpenSSH public key line pasted from the laptop.
///
/// # Outputs
///
/// [`HostTrustAppendResult`] describing success, duplicate skip, or sudo fallback.
pub fn host_append_authorized_pubkey_impl(pubkey_line: String) -> Result<HostTrustAppendResult, String> {
    let line = normalize_pubkey_line(&pubkey_line)?;
    validate_pubkey_line(&line)?;

    let (ssh_user, ak_path) = authorized_keys_path()?;
    let sudo_fallback = || {
        Ok(HostTrustAppendResult {
            appended: false,
            skipped_duplicate: false,
            pubkey_line_count: count_existing(&ak_path),
            sudo_script: Some(build_sudo_append_snippet(&ssh_user, &line, &ak_path)),
            message: "Cannot write authorized_keys as this user — run the sudo commands below.".into(),
        })
    };

    let mut existing = if ak_path.is_file() {
        std::fs::read_to_string(&ak_path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };

    if existing.lines().any(|l| {
        let t = l.trim();
        validate_pubkey_line(t).is_ok() && t == line.as_str()
    }) {
        return Ok(HostTrustAppendResult {
            appended: false,
            skipped_duplicate: true,
            pubkey_line_count: count_pubkey_lines(&existing),
            sudo_script: None,
            message: "That public key is already present.".into(),
        });
    }

    match OpenOptions::new().create(true).append(true).open(&ak_path) {
        Ok(mut f) => {
            if !existing.is_empty() && !existing.ends_with('\n') {
                f.write_all(b"\n").map_err(|e| e.to_string())?;
                existing.push('\n');
            }
            f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
            f.write_all(b"\n").map_err(|e| e.to_string())?;
            existing.push_str(&line);
            existing.push('\n');
            Ok(HostTrustAppendResult {
                appended: true,
                skipped_duplicate: false,
                pubkey_line_count: count_pubkey_lines(&existing),
                sudo_script: None,
                message: "Public key added to authorized_keys.".into(),
            })
        }
        Err(_) => {
            // Direct write failed — typical when Backr runs as the desktop user but
            // authorized_keys is owned by the backup system user.  Try the NOPASSWD
            // sudo tee rule written by the host install script before falling back to
            // the manual snippet.
            if sudo_tee_append(&line, &ak_path).is_ok() {
                let after = std::fs::read_to_string(&ak_path).unwrap_or_else(|_| {
                    let mut s = existing.clone();
                    s.push_str(&line);
                    s.push('\n');
                    s
                });
                return Ok(HostTrustAppendResult {
                    appended: true,
                    skipped_duplicate: false,
                    pubkey_line_count: count_pubkey_lines(&after),
                    sudo_script: None,
                    message: "Public key added to authorized_keys.".into(),
                });
            }
            sudo_fallback()
        }
    }
}

/// Appends `line` to `ak_path` via `sudo tee -a` using the NOPASSWD rule written by
/// the host install script (`/etc/sudoers.d/backr-trust-keys`).
///
/// # Inputs
///
/// * `line` — validated pubkey line.
/// * `ak_path` — absolute path to `authorized_keys`.
fn sudo_tee_append(line: &str, ak_path: &Path) -> Result<(), String> {
    let tee = Command::new("which")
        .arg("tee")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/bin/tee".to_string());

    let mut child = Command::new("sudo")
        .args([tee.as_str(), "-a", &ak_path.to_string_lossy()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("sudo tee spawn: {e}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
        writeln!(stdin, "{line}").map_err(|e| format!("sudo tee write: {e}"))?;
    }

    child
        .wait()
        .map_err(|e| e.to_string())
        .and_then(|s| if s.success() { Ok(()) } else { Err("sudo tee exited non-zero".into()) })
}

fn count_existing(ak_path: &Path) -> usize {
    if !ak_path.is_file() {
        return 0;
    }
    std::fs::read_to_string(ak_path)
        .map(|s| count_pubkey_lines(&s))
        .unwrap_or(0)
}

/// One parsed entry from `authorized_keys`, suitable for the host Settings key list.
#[derive(Debug, Serialize)]
pub struct AuthorizedPubkeyEntry {
    /// OpenSSH key type token, e.g. `ssh-ed25519`.
    pub key_type: String,
    /// Base-64 key material (middle column).
    pub key_b64: String,
    /// Optional trailing comment, typically `user@machine`.
    pub comment: String,
    /// The full raw line from the file — used as the stable identity when removing.
    pub raw_line: String,
}

/// Result of removing one pubkey entry from `authorized_keys`.
#[derive(Debug, Serialize)]
pub struct HostRemovePubkeyResult {
    /// True when the line was found and removed.
    pub removed: bool,
    /// Number of remaining pubkey lines after the operation.
    pub pubkey_line_count: u32,
}

/// Parses all valid pubkey lines from `authorized_keys` content into structured entries.
///
/// # Inputs
///
/// * `content` — full text of the `authorized_keys` file.
fn parse_pubkey_entries(content: &str) -> Vec<AuthorizedPubkeyEntry> {
    content
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            if validate_pubkey_line(t).is_err() {
                return None;
            }
            let mut parts = t.splitn(3, ' ');
            let key_type = parts.next()?.to_string();
            let key_b64 = parts.next()?.to_string();
            let comment = parts.next().unwrap_or("").trim().to_string();
            Some(AuthorizedPubkeyEntry {
                key_type,
                key_b64,
                comment,
                raw_line: t.to_string(),
            })
        })
        .collect()
}

/// Lists every parsed pubkey entry in `authorized_keys` for the host Settings view.
///
/// # Outputs
///
/// Vec of [`AuthorizedPubkeyEntry`] (empty when the file is absent or has no valid keys).
pub fn host_list_authorized_pubkeys_impl() -> Result<Vec<AuthorizedPubkeyEntry>, String> {
    let (_ssh_user, ak_path) = authorized_keys_path()?;
    let content = if ak_path.is_file() {
        std::fs::read_to_string(&ak_path).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    Ok(parse_pubkey_entries(&content))
}

/// Removes the line matching `raw_line` exactly from `authorized_keys`.
///
/// # Inputs
///
/// * `raw_line` — the exact full line string returned by [`host_list_authorized_pubkeys_impl`].
///
/// # Outputs
///
/// [`HostRemovePubkeyResult`] indicating whether the line was found and how many keys remain.
pub fn host_remove_authorized_pubkey_impl(raw_line: String) -> Result<HostRemovePubkeyResult, String> {
    let (_ssh_user, ak_path) = authorized_keys_path()?;

    let content = if ak_path.is_file() {
        std::fs::read_to_string(&ak_path).map_err(|e| e.to_string())?
    } else {
        return Ok(HostRemovePubkeyResult { removed: false, pubkey_line_count: 0 });
    };

    let target = raw_line.trim();
    let (kept, removed): (Vec<&str>, Vec<&str>) = content
        .lines()
        .partition(|l| l.trim() != target);

    if removed.is_empty() {
        return Ok(HostRemovePubkeyResult {
            removed: false,
            pubkey_line_count: count_pubkey_lines(&content) as u32,
        });
    }

    let mut new_content = kept.join("\n");
    if !new_content.is_empty() {
        new_content.push('\n');
    }

    std::fs::write(&ak_path, &new_content).map_err(|e| e.to_string())?;

    Ok(HostRemovePubkeyResult {
        removed: true,
        pubkey_line_count: count_pubkey_lines(&new_content) as u32,
    })
}
