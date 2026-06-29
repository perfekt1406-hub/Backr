/*
 * mDNS service discovery for one-tap pairing.
 *
 * Advertise side (host): registers `_backr._tcp` while in pairing mode so laptops
 * can find this host, auto-detecting LAN addresses. Browse side (client): lists
 * hosts currently advertising that service.
 */

use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};

use crate::pairing::PAIRING_SERVICE_TYPE;

/// A host found on the LAN that is currently in pairing mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Returns the primary outbound IPv4 address by connecting a UDP socket to an
/// external address (no packets are sent). Falls back to `None` when offline.
fn primary_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(a) if !a.is_loopback() => Some(a),
        _ => None,
    }
}

/// Advertises this host's pairing service on `port` over mDNS.
/// Registers only the primary IPv4 address so clients see one entry per host.
/// Falls back to auto-detection when no IPv4 address can be determined.
pub fn advertise(port: u16) -> Result<(ServiceDaemon, String), String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let host = hostname_short();
    let host_name = format!("{host}.local.");
    let instance = format!("Backr on {host}");
    let props = [("hostname", host.as_str())];

    let info = if let Some(ipv4) = primary_ipv4() {
        ServiceInfo::new(PAIRING_SERVICE_TYPE, &instance, &host_name, IpAddr::V4(ipv4), port, &props[..])
            .map_err(|e| e.to_string())?
    } else {
        // No IPv4 route found — fall back to auto-detection.
        ServiceInfo::new(PAIRING_SERVICE_TYPE, &instance, &host_name, "", port, &props[..])
            .map_err(|e| e.to_string())?
            .enable_addr_auto()
    };

    let fullname = info.get_fullname().to_string();
    daemon.register(info).map_err(|e| e.to_string())?;
    Ok((daemon, fullname))
}

/// Browses the LAN for hosts in pairing mode for up to `timeout`, de-duplicated by hostname.
/// When a host advertises both IPv4 and IPv6 addresses, IPv4 is preferred.
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
                let addrs = info.get_addresses();
                // Prefer IPv4 so we don't surface a long IPv6 address when a clean
                // IPv4 one is available for the same host.
                let best = addrs.iter().find(|a| a.is_ipv4()).or_else(|| addrs.iter().next());
                let Some(addr) = best else { continue };

                let address = format!("{addr}:{}", info.get_port());
                let hostname = info
                    .get_property_val_str("hostname")
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').to_string());

                // Deduplicate by hostname; upgrade an existing IPv6 entry to IPv4.
                if let Some(existing) = hosts.iter_mut().find(|h| h.hostname == hostname) {
                    if addr.is_ipv4() {
                        existing.address = address;
                    }
                } else {
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
