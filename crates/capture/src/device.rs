//! Enumerating Npcap-visible network adapters.

use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::os::raw::c_char;
use std::ptr;

use crate::error::Error;
use crate::ffi::{
    pcap_addr_t, pcap_if_t, RawSockaddr, SockaddrIn, AF_INET, PCAP_ERRBUF_SIZE, PCAP_IF_LOOPBACK,
    PCAP_IF_UP,
};
use crate::library::NpcapLibrary;

/// One network adapter as reported by `pcap_findalldevs`, copied out of libpcap's memory so it
/// can outlive the call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The name libpcap/Npcap uses to open this adapter, e.g. `\Device\NPF_{GUID}`.
    pub name: String,
    pub description: Option<String>,
    pub ipv4_addresses: Vec<Ipv4Addr>,
    pub is_loopback: bool,
    pub is_up: bool,
}

/// Lists every adapter Npcap can see.
pub fn list_devices(lib: &NpcapLibrary) -> Result<Vec<DeviceInfo>, Error> {
    let mut errbuf = [0 as c_char; PCAP_ERRBUF_SIZE];
    let mut head: *mut pcap_if_t = ptr::null_mut();

    // SAFETY: `head` and `errbuf` are valid out-parameters of the sizes `pcap_findalldevs`
    // expects; `lib.find_all_devs` is the real libpcap function resolved in `NpcapLibrary::load`.
    let rc = unsafe { (lib.find_all_devs)(&mut head, errbuf.as_mut_ptr()) };
    if rc != 0 {
        return Err(Error::Pcap {
            operation: "pcap_findalldevs",
            message: errbuf_to_string(&errbuf),
        });
    }

    // SAFETY: on success, `head` is either null (no devices) or the first node of a well-formed
    // linked list that stays valid until `pcap_freealldevs` is called below.
    let devices = unsafe { collect_devices(head) };

    // SAFETY: `head` is exactly the pointer `pcap_findalldevs` populated above, handed to the
    // matching `pcap_freealldevs` exactly once, and not read again afterward.
    unsafe { (lib.free_all_devs)(head) };

    Ok(devices)
}

/// # Safety
/// `head` must be null or point to a valid `pcap_if_t` linked list as produced by
/// `pcap_findalldevs`, valid for the duration of this call.
unsafe fn collect_devices(head: *mut pcap_if_t) -> Vec<DeviceInfo> {
    let mut devices = Vec::new();
    let mut node = head;
    while !node.is_null() {
        // SAFETY: `node` is non-null and, per this function's contract, points at a live
        // `pcap_if_t` whose `addresses` list (if any) is also valid.
        let dev = unsafe { &*node };
        devices.push(DeviceInfo {
            name: c_str_to_string(dev.name),
            description: non_empty(c_str_to_string(dev.description)),
            ipv4_addresses: unsafe { collect_ipv4_addresses(dev.addresses) },
            is_loopback: dev.flags & PCAP_IF_LOOPBACK != 0,
            is_up: dev.flags & PCAP_IF_UP != 0,
        });
        node = dev.next;
    }
    devices
}

/// # Safety
/// `head` must be null or point to a valid `pcap_addr_t` linked list.
unsafe fn collect_ipv4_addresses(head: *mut pcap_addr_t) -> Vec<Ipv4Addr> {
    let mut addrs = Vec::new();
    let mut node = head;
    while !node.is_null() {
        // SAFETY: per this function's contract, `node` points at a live `pcap_addr_t`.
        let entry = unsafe { &*node };
        // SAFETY: `entry.addr` is either null or a valid `sockaddr`, per the same contract.
        if let Some(ip) = unsafe { sockaddr_to_ipv4(entry.addr) } {
            addrs.push(ip);
        }
        node = entry.next;
    }
    addrs
}

/// # Safety
/// `addr` must be null or point to a valid, readable `RawSockaddr`.
unsafe fn sockaddr_to_ipv4(addr: *mut RawSockaddr) -> Option<Ipv4Addr> {
    if addr.is_null() {
        return None;
    }
    // SAFETY: non-null and valid per this function's contract.
    let family = unsafe { (*addr).sa_family };
    if family != AF_INET {
        return None;
    }
    // SAFETY: `SockaddrIn` and `RawSockaddr` are both 16-byte, 2-byte-aligned layouts overlaying
    // the same Winsock `sockaddr`; `family == AF_INET` confirms the OS actually populated this
    // one as a `sockaddr_in`, so reinterpreting the same bytes is valid.
    let as_in = unsafe { &*(addr as *const SockaddrIn) };
    Some(Ipv4Addr::from(as_in.sin_addr))
}

fn c_str_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: libpcap documents this field as a NUL-terminated C string when non-null, valid
    // for the duration of this call.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn errbuf_to_string(buf: &[c_char; PCAP_ERRBUF_SIZE]) -> String {
    // SAFETY: `buf` is a local, fully zero-initialized buffer that libpcap NUL-terminates on
    // error, so it is always a valid, in-bounds C string.
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
