/*
 * Client-side pairing.
 *
 * Ensures this laptop has an SSH key, sends its public key + the 6-digit code to a
 * discovered host's pairing listener, and returns a `PairDraft` containing the
 * prefilled Config draft AND the host's SSH key fingerprint for out-of-band user
 * verification. The config is NOT persisted here — the caller must call
 * `confirm_pairing` after the user confirms the fingerprint.
 *
 * The HTTP client is hand-rolled (both ends are ours; plain HTTP/1.1 with Connection: close).
 * Known-host pinning is deferred to `confirm_pair_draft` so it only happens on confirmed pairs.
 */

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{
    known_hosts_path, Config, LocalConfig, RemoteConfig, ScheduleConfig, StateConfig, UpdateConfig,
    CONFIG_VERSION,
};
use crate::pairing::discovery::hostname_short;

/// The host's reply to a successful pair (mirrors listener::HostPairInfo).
#[derive(Debug, Deserialize)]
struct PairReply {
    ssh_user: String,
    ssh_port: u16,
    backup_root: String,
    #[serde(default)]
    host_pubkey: String,
    /// SHA256 fingerprint string (e.g. `SHA256:abc...`) for user verification.
    #[serde(default)]
    host_key_fingerprint: String,
}

/// Intermediate result of a successful pair POST: the prefilled config draft and the
/// host's SSH key fingerprint so the user can verify authenticity before the config
/// is saved. Pass the entire struct to `confirm_pair_draft` after user confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairDraft {
    /// The prefilled config to be saved after user confirms the fingerprint.
    pub config: Config,
    /// SHA256 fingerprint of the host's SSH key (e.g. `SHA256:abc...`).
    /// Should match what is displayed on the host's Backr screen.
    pub host_key_fingerprint: String,
    /// Full SSH host public key line to pin into known_hosts on confirmation.
    /// Kept here so the pin step and the config-save step are atomic from the UI's view.
    pub host_pubkey: String,
    /// The resolved SSH target (IP or mDNS name) the client will connect to.
    pub ssh_target: String,
}

/// Pairs with a discovered host at `address` ("ip:port") using `code`: ensures a local
/// SSH key, submits the public key, and returns a `PairDraft` containing the prefilled
/// config and the host's SSH key fingerprint for user verification. Known-host pinning
/// is deferred to `confirm_pair_draft` so it only happens after the user confirms.
///
/// # Inputs
///
/// * `address` — "ip:port" of the host's pairing listener, from `discover_hosts`.
/// * `code`    — 6-digit pairing code shown on the host screen.
///
/// # Returns
///
/// `PairDraft` on success, or a human-readable error string on failure.
pub fn pair_with_host(address: &str, code: &str) -> Result<PairDraft, String> {
    let pubkey = ensure_ssh_key()?;
    let key_path = ssh_key_path()?.to_string_lossy().to_string();

    let body = serde_json::json!({ "pubkey": pubkey, "code": code }).to_string();
    let (status, resp_body) = http_post_json(address, "/pair", &body)?;
    match status {
        200 => {}
        400 => return Err("Host rejected the public key.".into()),
        403 => return Err("Incorrect or expired code.".into()),
        409 => return Err("Host is not in pairing mode.".into()),
        c => return Err(format!("Host rejected pairing (HTTP {c}).")),
    }
    let reply: PairReply =
        serde_json::from_str(&resp_body).map_err(|e| format!("bad pair reply: {e}"))?;

    // Use the IP the pairing request actually reached. The host advertises its
    // `<hostname>.local` mDNS name ONLY while in pairing mode, so a "resolves now" check
    // is a false positive: the name resolves during pairing (the host's mDNS responder
    // is up and answering) but stops resolving once pairing ends — which then fails every
    // backup with "Could not resolve hostname ….local". The paired IP is verified
    // reachable (we just POSTed to it) and stable on this LAN; if it changes via DHCP,
    // re-pair or edit the host under Settings.
    let ssh_target = address.split(':').next().unwrap_or(address).to_string();

    // Build the config draft but do NOT pin the known_host or save yet — the user must
    // first verify the fingerprint shown here matches what is displayed on the host screen.
    let config = Config {
        version: CONFIG_VERSION,
        remote: RemoteConfig {
            host: ssh_target.clone(),
            user: reply.ssh_user,
            ssh_key: key_path,
            port: reply.ssh_port,
            backup_path: reply.backup_root,
        },
        local: LocalConfig {
            projects_path: default_projects_path(),
        },
        schedule: ScheduleConfig { interval_hours: 3 },
        state: StateConfig {
            last_backup_at: None,
        },
        update: UpdateConfig::default(),
    };

    Ok(PairDraft {
        config,
        host_key_fingerprint: reply.host_key_fingerprint,
        host_pubkey: reply.host_pubkey,
        ssh_target,
    })
}

/// Finalizes a confirmed pair: pins the host public key into known_hosts and returns the
/// ready-to-save config. Call this ONLY after the user has verified the fingerprint shown
/// in `PairDraft::host_key_fingerprint` against the host screen.
///
/// # Inputs
///
/// * `draft` — the `PairDraft` returned by `pair_with_host`.
///
/// # Returns
///
/// The finalized `Config` on success, or a human-readable error string on failure.
pub fn confirm_pair_draft(draft: PairDraft) -> Result<Config, String> {
    // Pin the host key under the exact target we'll connect with so StrictHostKeyChecking
    // verifies regardless of the host's current IP.
    if !draft.host_pubkey.trim().is_empty() {
        pin_known_host(&draft.ssh_target, draft.host_pubkey.trim())?;
    }
    Ok(draft.config)
}

fn ssh_key_path() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("could not resolve home directory")?;
    Ok(home.join(".ssh").join("id_ed25519"))
}

fn default_projects_path() -> String {
    dirs::home_dir()
        .map(|h| h.join("Projects").to_string_lossy().to_string())
        .unwrap_or_else(|| "~/Projects".into())
}

/// Returns the laptop's public key line, generating a passphraseless ed25519 key if missing.
fn ensure_ssh_key() -> Result<String, String> {
    let priv_path = ssh_key_path()?;
    let pub_path = priv_path.with_extension("pub");
    if !priv_path.exists() {
        if let Some(dir) = priv_path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        let comment = format!("backr-{user}@{}", hostname_short());
        let status = Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-f"])
            .arg(&priv_path)
            .arg("-C")
            .arg(&comment)
            .status()
            .map_err(|e| format!("ssh-keygen failed: {e}"))?;
        if !status.success() {
            return Err("ssh-keygen did not succeed".into());
        }
    }
    let line = fs::read_to_string(&pub_path).map_err(|e| format!("read public key: {e}"))?;
    Ok(line.trim().to_string())
}

/// Appends `<host> <host_pubkey>` to the isolated known_hosts unless already present.
fn pin_known_host(host: &str, host_pubkey: &str) -> Result<(), String> {
    let path = known_hosts_path().map_err(|e| e.to_string())?;
    let entry = format!("{host} {host_pubkey}");
    let existing = fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(f, "{entry}").map_err(|e| e.to_string())?;
    Ok(())
}

/// Minimal HTTP/1.1 POST for the pairing request. Both ends are ours and the listener
/// replies with `Connection: close`, so the body is everything after the header block.
fn http_post_json(address: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let mut stream =
        TcpStream::connect(address).map_err(|e| format!("could not reach host: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\n\
         Content-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        len = body.len()
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (head, resp_body) = text
        .split_once("\r\n\r\n")
        .ok_or("malformed HTTP response")?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("could not parse HTTP status")?;
    Ok((status, resp_body.to_string()))
}
