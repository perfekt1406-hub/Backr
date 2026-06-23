/*
 * mDNS service discovery for one-tap pairing.
 *
 * Advertise side (host): registers `_backr._tcp` while in pairing mode so laptops
 * can find this host, auto-detecting LAN addresses. Browse side (client): lists
 * hosts currently advertising that service.
 */

use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

use crate::pairing::PAIRING_SERVICE_TYPE;

/// A host found on the LAN that is currently in pairing mode.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredHost {
    /// Friendly hostname for display.
    pub hostname: String,
    /// "ip:port" the client connects to for the pairing request.
    pub address: String,
}

/// Short hostname used for the service instance name and TXT record.
pub fn hostname_short() -> String {
    std::process::Command::new("hostname")
        .arg("-s")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "backr-host".to_string())
}

/// Advertises this host's pairing service on `port` over mDNS, auto-detecting LAN
/// addresses. Returns the daemon and the registered fullname for later unregister.
pub fn advertise(port: u16) -> Result<(ServiceDaemon, String), String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let host = hostname_short();
    let host_name = format!("{host}.local.");
    let instance = format!("Backr on {host}");
    let props = [("hostname", host.as_str())];
    let info = ServiceInfo::new(PAIRING_SERVICE_TYPE, &instance, &host_name, "", port, &props[..])
        .map_err(|e| e.to_string())?
        .enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon.register(info).map_err(|e| e.to_string())?;
    Ok((daemon, fullname))
}

/// Browses the LAN for hosts in pairing mode for up to `timeout`, de-duplicated by address.
pub fn discover_hosts(timeout: Duration) -> Result<Vec<DiscoveredHost>, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let receiver = daemon
        .browse(PAIRING_SERVICE_TYPE)
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    let mut hosts: Vec<DiscoveredHost> = Vec::new();

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match receiver.recv_timeout(remaining) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                if let Some(addr) = info.get_addresses().iter().next() {
                    let address = format!("{addr}:{}", info.get_port());
                    if hosts.iter().any(|h| h.address == address) {
                        continue;
                    }
                    let hostname = info
                        .get_property_val_str("hostname")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').to_string());
                    hosts.push(DiscoveredHost { hostname, address });
                }
            }
            Ok(_) => {}
            Err(_) => break, // timeout window reached
        }
    }

    let _ = daemon.shutdown();
    Ok(hosts)
}
