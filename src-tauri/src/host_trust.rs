/*
 * Purpose: Backup-host «Trust keys» flow — inspect authorized_keys and append laptop pubkey lines from the UI.
 * Role: Used only when Backr boots in host-dashboard mode (`/etc/backr/host.toml`). Writes are best-effort;
 *       falls back to a sudo shell snippet when the desktop user cannot write `~backup/.ssh/authorized_keys`.
 *       Reads also use `sudo -n /bin/cat` via the same privilege-escalation path already established for
 *       writes, so the host dashboard always shows the real trusted-key count.
 *
 * Security note on the privileged helper:
 *   The sudoers drop-in installed by `setup-backup-host.sh` (`/etc/sudoers.d/10-backr-trust`) grants
 *   NOPASSWD access to exactly one binary: `/usr/local/lib/backr/append-trusted-key`.  It is *not*
 *   `NOPASSWD: ALL` and is scoped to root.  The helper itself validates the incoming public-key line
 *   against a strict allowlist of OpenSSH key-type prefixes before touching `authorized_keys`, so it
 *   rejects any other content.  A second, narrower sudo rule for `/bin/cat` on the `authorized_keys`
 *   path can be added for reads; until then the read path falls back to `sudo -n /bin/cat` which is
 *   trusted only when the operator has arranged a matching NOPASSWD rule for it.
 */

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Reads the `authorized_keys` file via `sudo -n /bin/cat` and returns the count of valid pubkey lines.
///
/// The Backr host app runs as the desktop user who cannot read the backup user's `~/.ssh/authorized_keys`
/// (owned by the backup system user, mode 600).  We use `sudo -n /bin/cat` with a non-interactive flag
/// so it fails fast rather than prompting when no NOPASSWD rule is present.  The operator should have a
/// sudoers rule allowing this; see the file-level security note for the scope of that rule.
///
/// # Inputs
///
/// * `ak_path` — absolute path to the `authorized_keys` file.
///
/// # Outputs
///
/// Number of valid pubkey lines, or `0` on any error (sudo unavailable, file absent, permission denied).
/// Errors are logged at `warn` level so they are visible in diagnostics without surfacing to the UI.
fn count_trusted_keys_impl(ak_path: &Path) -> usize {
    // Attempt direct read first (works when the desktop user happens to own the file, e.g. in tests).
    if let Ok(content) = std::fs::read_to_string(ak_path) {
        return count_pubkey_lines(&content);
    }

    // Fall back to sudo -n /bin/cat so the read succeeds when a NOPASSWD rule for this path is configured.
    // `-n` keeps sudo non-interactive: it exits with an error rather than prompting for a password when
    // no NOPASSWD rule covers this binary + path combination.
    let path_str = ak_path.to_string_lossy();
    let result = Command::new("sudo")
        .args(["-n", "/bin/cat", path_str.as_ref()])
        .output();

    match result {
        Ok(out) if out.status.success() => {
            let content = String::from_utf8_lossy(&out.stdout);
            count_pubkey_lines(&content)
        }
        Ok(out) => {
            // sudo failed (no NOPASSWD rule, file missing, etc.) — degrade gracefully.
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(
                "count_trusted_keys_impl: sudo cat authorized_keys failed (status {:?}): {}",
                out.status.code(),
                stderr.trim()
            );
            0
        }
        Err(e) => {
            tracing::warn!("count_trusted_keys_impl: could not run sudo: {e}");
            0
        }
    }
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

/// Absolute path to the privileged helper installed by `setup-backup-host.sh`.
/// The host app runs as the desktop user, which cannot write the backup user's
/// `authorized_keys` directly; this helper (invoked via passwordless `sudo -n`)
/// performs the append as root so one-tap pairing needs no manual key trust.
const TRUST_HELPER_PATH: &str = "/usr/local/lib/backr/append-trusted-key";

/// Appends `line` to the backup user's `authorized_keys` via the privileged helper.
///
/// # Inputs
///
/// * `line` — validated OpenSSH public key line.
///
/// # Outputs
///
/// `Ok(())` when `sudo -n <helper>` exits 0; `Err` with stderr/context otherwise
/// (helper missing, no passwordless sudo rule, or the append failed).
fn try_privileged_helper_append(line: &str) -> Result<(), String> {
    if !Path::new(TRUST_HELPER_PATH).is_file() {
        return Err(format!("trust helper not installed at {TRUST_HELPER_PATH}"));
    }
    // `-n` keeps sudo non-interactive: it fails fast instead of prompting for a
    // password when the NOPASSWD rule is absent.  The key is piped on stdin so it
    // never appears in the process list / sudo audit args.
    let mut child = Command::new("sudo")
        .args(["-n", TRUST_HELPER_PATH])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run sudo helper: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| format!("could not write key to helper: {e}"))?;
        // Drop stdin to send EOF so the helper's `cat`/read returns.
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("sudo helper did not complete: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "sudo helper failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
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
#[derive(Debug, Serialize, serde::Deserialize)]
pub struct HostTrustStatus {
    pub ssh_user: String,
    pub authorized_keys_path: String,
    pub pubkey_line_count: usize,
}

/// Result row for [`super::host_cmd::host_append_authorized_pubkey`].
#[derive(Debug, Serialize, serde::Deserialize)]
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
/// Uses [`count_trusted_keys_impl`] to obtain the real key count via `sudo -n /bin/cat` when the
/// desktop user cannot read the backup user's `authorized_keys` directly.  Falls back to `0` without
/// panicking when privilege escalation is unavailable.
///
/// # Outputs
///
/// [`HostTrustStatus`] or error when host marker / passwd lookup fails.
pub fn host_trust_status_impl() -> Result<HostTrustStatus, String> {
    let (ssh_user, ak_path) = authorized_keys_path()?;
    let path_str = ak_path.to_string_lossy().to_string();
    // count_trusted_keys_impl tries a direct read first, then escalates via sudo -n /bin/cat.
    // It never panics; it returns 0 and logs a warning when escalation is unavailable.
    let pubkey_line_count = count_trusted_keys_impl(&ak_path);
    Ok(HostTrustStatus {
        ssh_user,
        authorized_keys_path: path_str,
        pubkey_line_count,
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
        // Direct write failed (host app runs as the desktop user, authorized_keys is
        // owned by the backup user).  Escalate through the privileged helper so pairing
        // stays hands-off; only when that also fails do we surface the manual snippet.
        Err(_) => match try_privileged_helper_append(&line) {
            Ok(()) => Ok(HostTrustAppendResult {
                appended: true,
                skipped_duplicate: false,
                pubkey_line_count: count_existing(&ak_path),
                sudo_script: None,
                message: "Public key added to authorized_keys (via privileged helper).".into(),
            }),
            Err(helper_err) => {
                tracing::warn!("trust helper append failed: {helper_err}");
                sudo_fallback()
            }
        },
    }
}

/// Returns the current trusted-key count after a write operation (append or helper-write).
///
/// Uses `count_trusted_keys_impl` so the post-write count reflects the actual file content
/// even when the desktop user cannot read the backup user's `authorized_keys` directly.
///
/// # Inputs
///
/// * `ak_path` — absolute path to the `authorized_keys` file.
fn count_existing(ak_path: &Path) -> usize {
    count_trusted_keys_impl(ak_path)
}

/// One parsed entry from `authorized_keys`, suitable for the host Settings key list.
#[derive(Debug, Serialize, serde::Deserialize)]
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
#[derive(Debug, Serialize, serde::Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Returns a unique path inside the system temp directory for test isolation.
    fn tmp_path(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "backr_trust_test_{}_{}",
            suffix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    /// `count_trusted_keys_impl` with a non-existent file returns 0 without panicking.
    ///
    /// This exercises the graceful-degradation path: no file → `read_to_string` fails →
    /// `sudo -n /bin/cat` also fails or is not available → returns 0.
    #[test]
    fn count_trusted_keys_absent_file_returns_zero() {
        let path = tmp_path("absent");
        // The file must not exist for this test.
        assert!(!path.exists(), "temp path should not exist before test");
        let count = count_trusted_keys_impl(&path);
        assert_eq!(count, 0, "absent authorized_keys should yield 0 keys");
    }

    /// `count_trusted_keys_impl` with a readable file counts only valid pubkey lines.
    ///
    /// When the desktop user *can* read the file (e.g. in a test or if ownership matches),
    /// the function must return the correct count without involving sudo.
    #[test]
    fn count_trusted_keys_readable_file_returns_correct_count() {
        let path = tmp_path("readable");
        let content = "# comment — should not be counted\n\
                        \n\
                        ssh-ed25519 AAAA1111 user@laptop1\n\
                        ssh-rsa AAAA2222 user@laptop2\n\
                        not-a-real-key this should be ignored\n";
        fs::write(&path, content).expect("write test authorized_keys");

        let count = count_trusted_keys_impl(&path);
        // Cleanup before assertion so the file is removed even if the assert fails.
        let _ = fs::remove_file(&path);

        assert_eq!(count, 2, "should count exactly the two valid pubkey lines");
    }

    /// `count_pubkey_lines` correctly skips blanks and comments.
    #[test]
    fn count_pubkey_lines_skips_blanks_and_comments() {
        let content = "\n# a comment\nssh-ed25519 AAAA user@host\n\nssh-rsa BBBB user2@host\n";
        assert_eq!(count_pubkey_lines(content), 2);
    }

    /// `normalize_pubkey_line` strips Windows CRLF and leading/trailing whitespace.
    #[test]
    fn normalize_pubkey_line_strips_crlf_and_whitespace() {
        let raw = "  ssh-ed25519 AAAA user@host\r\n";
        let got = normalize_pubkey_line(raw).unwrap();
        assert_eq!(got, "ssh-ed25519 AAAA user@host");
    }

    /// `validate_pubkey_line` rejects comments.
    #[test]
    fn validate_pubkey_line_rejects_comments() {
        assert!(validate_pubkey_line("# not a key").is_err());
    }

    /// `validate_pubkey_line` accepts known key type prefixes.
    #[test]
    fn validate_pubkey_line_accepts_known_prefixes() {
        assert!(validate_pubkey_line("ssh-ed25519 AAAA user@host").is_ok());
        assert!(validate_pubkey_line("ssh-rsa BBBB user@host").is_ok());
        assert!(validate_pubkey_line("ecdsa-sha2-nistp256 CCCC user@host").is_ok());
    }
}
