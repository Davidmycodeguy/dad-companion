//! Choosing which network adapter to capture on.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

use crate::device::DeviceInfo;
use crate::error::Error;

/// A public address used only to ask the OS routing table which local address/adapter it would
/// use to reach the internet. Nothing is ever sent here: UDP `connect` just records a peer
/// address and asks the kernel to pick a source address and route for it.
const ROUTE_PROBE_ADDR: &str = "8.8.8.8:80";

/// The local IPv4 address the OS would use to reach the public internet: connect a UDP socket to
/// a public address and read back the source address the kernel picked for the route. This
/// never actually transmits a packet (UDP `connect` is purely a local, kernel-side operation).
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(ROUTE_PROBE_ADDR).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

/// Picks which adapter to capture on.
///
/// Priority: an explicit `override_name` always wins (and is an error if it doesn't match any
/// adapter); otherwise prefer whichever adapter carries `local_ip` (the default-route
/// interface); otherwise fall back to the first up, non-loopback adapter.
pub fn choose_adapter<'a>(
    devices: &'a [DeviceInfo],
    override_name: Option<&str>,
    local_ip: Option<Ipv4Addr>,
) -> Result<&'a DeviceInfo, Error> {
    if let Some(name) = override_name {
        return devices
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| Error::AdapterNotFound(name.to_string()));
    }

    if let Some(ip) = local_ip {
        if let Some(found) = devices.iter().find(|d| d.ipv4_addresses.contains(&ip)) {
            return Ok(found);
        }
    }

    devices
        .iter()
        .find(|d| d.is_up && !d.is_loopback)
        .ok_or(Error::NoAdapterFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, ips: &[&str], up: bool, loopback: bool) -> DeviceInfo {
        DeviceInfo {
            name: name.to_string(),
            description: None,
            ipv4_addresses: ips.iter().map(|s| s.parse().unwrap()).collect(),
            is_loopback: loopback,
            is_up: up,
        }
    }

    #[test]
    fn picks_the_adapter_matching_the_local_ip() {
        let devices = vec![
            device("eth0", &["10.0.0.5"], true, false),
            device("wifi0", &["192.168.1.20"], true, false),
        ];
        let ip = "192.168.1.20".parse().unwrap();
        let chosen = choose_adapter(&devices, None, Some(ip)).unwrap();
        assert_eq!(chosen.name, "wifi0");
    }

    #[test]
    fn falls_back_to_first_up_non_loopback_when_no_ip_matches() {
        let devices = vec![
            device("loop", &["127.0.0.1"], true, true),
            device("down0", &[], false, false),
            device("eth0", &["10.0.0.5"], true, false),
        ];
        let unmatched = "9.9.9.9".parse().unwrap();
        let chosen = choose_adapter(&devices, None, Some(unmatched)).unwrap();
        assert_eq!(chosen.name, "eth0");
    }

    #[test]
    fn explicit_override_wins_even_if_local_ip_matches_another() {
        let devices = vec![
            device("eth0", &["10.0.0.5"], true, false),
            device("wifi0", &["192.168.1.20"], true, false),
        ];
        let ip = "192.168.1.20".parse().unwrap();
        let chosen = choose_adapter(&devices, Some("eth0"), Some(ip)).unwrap();
        assert_eq!(chosen.name, "eth0");
    }

    #[test]
    fn explicit_override_not_found_is_an_error() {
        let devices = vec![device("eth0", &["10.0.0.5"], true, false)];
        let err = choose_adapter(&devices, Some("nope"), None).unwrap_err();
        assert!(matches!(err, Error::AdapterNotFound(name) if name == "nope"));
    }

    #[test]
    fn no_usable_adapter_is_an_error() {
        let devices = vec![device("loop", &["127.0.0.1"], true, true)];
        let err = choose_adapter(&devices, None, None).unwrap_err();
        assert!(matches!(err, Error::NoAdapterFound));
    }
}
