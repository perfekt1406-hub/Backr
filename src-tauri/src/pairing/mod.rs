/*
 * One-tap LAN pairing.
 *
 * Lets a laptop discover this host over mDNS and get its SSH public key trusted
 * via a time-boxed 6-digit code, replacing the manual host-IP + pubkey-copy setup.
 * Primitives live here; the Tauri commands that drive them are in
 * `commands/pairing_cmd.rs`.
 */

pub mod client;
pub mod code;
pub mod discovery;
pub mod listener;

use std::sync::Arc;

use mdns_sd::ServiceDaemon;
use tiny_http::Server;

/// mDNS service type a host advertises while in pairing mode and a client browses for.
pub const PAIRING_SERVICE_TYPE: &str = "_backr._tcp.local.";

/// Live host-side pairing resources, torn down on stop, timeout, or a successful pair.
pub struct PairingRuntime {
    /// mDNS daemon advertising this host.
    pub mdns: ServiceDaemon,
    /// Registered service fullname, used to unregister on teardown.
    pub fullname: String,
    /// The HTTP pairing listener; `unblock()` ends its serve loop.
    pub server: Arc<Server>,
    /// The OS thread running the blocking serve loop.
    pub thread: Option<std::thread::JoinHandle<()>>,
}

impl PairingRuntime {
    /// Unregisters mDNS, stops the daemon, unblocks the serve loop, and joins its thread.
    pub fn shutdown(mut self) {
        let _ = self.mdns.unregister(&self.fullname);
        let _ = self.mdns.shutdown();
        self.server.unblock();
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}
