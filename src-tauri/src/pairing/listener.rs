/*
 * Host pairing listener.
 *
 * Temporary HTTP endpoint a host runs during pairing mode. It accepts
 * `POST /pair { pubkey, code }` from a discovered laptop: validates the 6-digit
 * code against the active PairingSession, trusts the laptop's public key through
 * the same path as the Trust-keys UI, and returns this host's connection details
 * so the laptop can prefill its setup. The start/stop lifecycle and mDNS
 * advertisement live in `commands/pairing_cmd.rs` (U4); this module holds the
 * request logic and the blocking serve loop.
 */

use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tiny_http::{Method, Response, Server};

use crate::host_config::read_host_dashboard_marker;
use crate::host_trust::{
    host_append_authorized_pubkey_impl, normalize_pubkey_line, validate_pubkey_line,
};
use crate::pairing::code::{CodeValidation, PairingSession};
use crate::state::AppState;

/// What a laptop POSTs to `/pair`.
#[derive(Debug, Deserialize)]
pub struct PairRequest {
    pub pubkey: String,
    pub code: String,
}

/// What the host returns on a successful pair — used to prefill the laptop's setup.
#[derive(Debug, Clone, Serialize)]
pub struct HostPairInfo {
    pub ssh_user: String,
    pub ssh_port: u16,
    pub backup_root: String,
    /// SHA256 fingerprint for display/verification.
    pub host_key_fingerprint: String,
    /// Full SSH host public key line the client pins into known_hosts.
    pub host_pubkey: String,
}

/// Why a pair attempt was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum PairRejection {
    BadCode(CodeValidation),
    BadPubkey(String),
    AppendFailed(String),
}

/// Pure pairing decision: validate the public key, consume the code, trust the key,
/// and return the host info. The pubkey is validated **before** the code is consumed
/// so a malformed key can't burn a valid code. `append` performs the real
/// authorized_keys write (injected so this stays unit-testable).
pub fn process_pair<F>(
    session: &mut PairingSession,
    req: &PairRequest,
    host: &HostPairInfo,
    append: F,
) -> Result<HostPairInfo, PairRejection>
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let line = normalize_pubkey_line(&req.pubkey)
        .and_then(|l| validate_pubkey_line(&l).map(|_| l))
        .map_err(PairRejection::BadPubkey)?;
    match session.validate(&req.code) {
        CodeValidation::Valid => {}
        other => return Err(PairRejection::BadCode(other)),
    }
    append(&line).map_err(PairRejection::AppendFailed)?;
    Ok(host.clone())
}

/// Gathers this host's connection details for the pair reply: SSH user + backup root
/// from the host marker, the effective sshd port, and the SSH host key fingerprint.
pub fn gather_host_info() -> Result<HostPairInfo, String> {
    let marker = read_host_dashboard_marker()
        .ok_or_else(|| "not a Backr host (no /etc/backr/host.toml)".to_string())?;
    let ssh_user = marker
        .ssh_user
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "backr".to_string());
    Ok(HostPairInfo {
        ssh_user,
        ssh_port: effective_sshd_port().unwrap_or(22),
        backup_root: marker.backup_root,
        host_key_fingerprint: host_key_fingerprint().unwrap_or_default(),
        host_pubkey: host_pubkey_line().unwrap_or_default(),
    })
}

/// Full SSH host public key line (for the client to pin into known_hosts).
fn host_pubkey_line() -> Option<String> {
    std::fs::read_to_string("/etc/ssh/ssh_host_ed25519_key.pub")
        .ok()
        .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Best-effort effective sshd port via `sshd -T` (caller falls back to 22).
fn effective_sshd_port() -> Option<u16> {
    let out = Command::new("sshd").arg("-T").output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("port "))
        .and_then(|p| p.trim().parse().ok())
}

/// SSH host key fingerprint (`SHA256:…`) from the ed25519 host key via `ssh-keygen -lf`.
fn host_key_fingerprint() -> Option<String> {
    let out = Command::new("ssh-keygen")
        .args(["-lf", "/etc/ssh/ssh_host_ed25519_key.pub"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // ssh-keygen -lf prints: "<bits> SHA256:<hash> <comment> (ED25519)".
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find(|tok| tok.starts_with("SHA256:"))
        .map(|s| s.to_string())
}

fn json_header() -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .expect("valid content-type header")
}

/// Blocking serve loop for the pairing window. Runs on a dedicated OS thread (never a
/// Tokio worker — it uses `blocking_lock`). Returns after one successful pair; the
/// caller (U4) also tears it down on timeout/cancel by dropping the server.
pub fn serve(server: Arc<Server>, state: Arc<AppState>, host: HostPairInfo) {
    let mut paired = false;
    for request in server.incoming_requests() {
        if handle_request(request, &state, &host) {
            paired = true;
            break;
        }
    }
    if paired {
        // A successful pair closes the window immediately: remove the runtime
        // (without joining our own thread) and stop advertising. A later TTL
        // teardown then finds nothing and no-ops.
        let runtime = state.pairing_runtime.blocking_lock().take();
        if let Some(rt) = runtime {
            let _ = rt.mdns.unregister(&rt.fullname);
            let _ = rt.mdns.shutdown();
            // rt.server + rt.thread (our own handle) drop here, detaching this thread.
        }
        *state.pairing.blocking_lock() = None;
    }
}

/// Handles one request; returns true when a successful pair occurred (ends the window).
fn handle_request(mut request: tiny_http::Request, state: &AppState, host: &HostPairInfo) -> bool {
    if request.method() != &Method::Post || request.url() != "/pair" {
        let _ = request.respond(Response::from_string("not found").with_status_code(404));
        return false;
    }
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        let _ = request.respond(Response::from_string("bad body").with_status_code(400));
        return false;
    }
    let req: PairRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => {
            let _ = request.respond(Response::from_string("bad json").with_status_code(400));
            return false;
        }
    };

    // Sync thread → blocking_lock on the Tokio mutex holding the active session.
    let mut guard = state.pairing.blocking_lock();
    let Some(session) = guard.as_mut() else {
        let _ = request.respond(Response::from_string("no active pairing").with_status_code(409));
        return false;
    };
    let result = process_pair(session, &req, host, |line| {
        let r = host_append_authorized_pubkey_impl(line.to_string())
            .map_err(|e| e)?;
        // appended=false + skipped_duplicate=false means the host process doesn't own
        // authorized_keys — the sudo fallback path returned Ok but wrote nothing.
        if r.appended || r.skipped_duplicate {
            Ok(())
        } else {
            Err("host cannot write authorized_keys — run the sudo snippet from the Trust keys UI".to_string())
        }
    });
    drop(guard);

    match result {
        Ok(info) => {
            let json = serde_json::to_string(&info).unwrap_or_default();
            let _ = request.respond(
                Response::from_string(json)
                    .with_header(json_header())
                    .with_status_code(200),
            );
            true
        }
        Err(rej) => {
            let (status, msg) = match rej {
                PairRejection::BadCode(_) => (403, "invalid or expired code"),
                PairRejection::BadPubkey(_) => (400, "invalid public key"),
                PairRejection::AppendFailed(_) => (500, "could not trust key"),
            };
            let _ = request.respond(Response::from_string(msg).with_status_code(status));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const VALID_PUBKEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITESTKEY00000000 test@laptop";

    fn host() -> HostPairInfo {
        HostPairInfo {
            ssh_user: "backr".into(),
            ssh_port: 22,
            backup_root: "/srv/backr".into(),
            host_key_fingerprint: "SHA256:abc".into(),
            host_pubkey: "ssh-ed25519 AAAAHOSTKEY host".into(),
        }
    }

    #[test]
    fn valid_code_and_key_trusts_and_returns_host_info() {
        let mut s = PairingSession::new();
        let req = PairRequest {
            pubkey: VALID_PUBKEY.into(),
            code: s.code().to_string(),
        };
        let appended = Cell::new(false);
        let reply = process_pair(&mut s, &req, &host(), |line| {
            assert_eq!(line, VALID_PUBKEY);
            appended.set(true);
            Ok(())
        })
        .expect("should pair");
        assert!(appended.get());
        assert_eq!(reply.ssh_user, "backr");
        assert_eq!(reply.backup_root, "/srv/backr");
        assert_eq!(reply.ssh_port, 22);
    }

    #[test]
    fn wrong_code_does_not_append() {
        let mut s = PairingSession::new();
        let wrong = if s.code() == "000000" { "111111" } else { "000000" };
        let req = PairRequest {
            pubkey: VALID_PUBKEY.into(),
            code: wrong.into(),
        };
        let appended = Cell::new(false);
        let res = process_pair(&mut s, &req, &host(), |_| {
            appended.set(true);
            Ok(())
        });
        assert!(matches!(res, Err(PairRejection::BadCode(_))));
        assert!(!appended.get());
    }

    #[test]
    fn malformed_pubkey_rejected_without_consuming_code() {
        let mut s = PairingSession::new();
        let code = s.code().to_string();
        let req = PairRequest {
            pubkey: "not-a-key".into(),
            code: code.clone(),
        };
        let appended = Cell::new(false);
        let res = process_pair(&mut s, &req, &host(), |_| {
            appended.set(true);
            Ok(())
        });
        assert!(matches!(res, Err(PairRejection::BadPubkey(_))));
        assert!(!appended.get());
        // Code was not consumed — a subsequent correct attempt still works.
        let req2 = PairRequest {
            pubkey: VALID_PUBKEY.into(),
            code,
        };
        assert!(process_pair(&mut s, &req2, &host(), |_| Ok(())).is_ok());
    }

    #[test]
    fn append_failure_surfaces() {
        let mut s = PairingSession::new();
        let req = PairRequest {
            pubkey: VALID_PUBKEY.into(),
            code: s.code().to_string(),
        };
        let res = process_pair(&mut s, &req, &host(), |_| Err("disk full".into()));
        assert!(matches!(res, Err(PairRejection::AppendFailed(_))));
    }
}
