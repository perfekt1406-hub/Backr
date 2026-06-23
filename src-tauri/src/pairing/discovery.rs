/*
 * mDNS service discovery for one-tap pairing.
 *
 * Advertise side (host, U4): registers `_backr._tcp` while in pairing mode so
 * laptops can find this host, auto-detecting LAN addresses. The browse side
 * (client discovery) is added in U5.
 */

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::pairing::PAIRING_SERVICE_TYPE;

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
