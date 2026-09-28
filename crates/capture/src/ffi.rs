//! Raw libpcap/Npcap ABI: struct layouts, constants and function-pointer types.
//!
//! Nothing here is unsafe to *define* (no function bodies), only to *call*; callers in
//! [`crate::library`] and [`crate::handle`] carry the `// SAFETY:` comments.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_uchar, c_void};

/// Opaque libpcap capture handle (`pcap_t*`). We never read its fields, only pass the pointer
/// back to libpcap functions.
pub type pcap_t = c_void;

#[repr(C)]
pub struct pcap_if_t {
    pub next: *mut pcap_if_t,
    pub name: *mut c_char,
    pub description: *mut c_char,
    pub addresses: *mut pcap_addr_t,
    pub flags: u32,
}

#[repr(C)]
pub struct pcap_addr_t {
    pub next: *mut pcap_addr_t,
    pub addr: *mut RawSockaddr,
    pub netmask: *mut RawSockaddr,
    pub broadaddr: *mut RawSockaddr,
    pub dstaddr: *mut RawSockaddr,
}

/// Generic `struct sockaddr` (Winsock): 2-byte family + 14 bytes of family-specific data.
#[repr(C)]
pub struct RawSockaddr {
    pub sa_family: u16,
    pub sa_data: [u8; 14],
}

/// `struct sockaddr_in`, overlaid on a [`RawSockaddr`] once `sa_family == AF_INET`.
#[repr(C)]
pub struct SockaddrIn {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

pub const AF_INET: u16 = 2;

pub const PCAP_IF_LOOPBACK: u32 = 0x0000_0001;
pub const PCAP_IF_UP: u32 = 0x0000_0002;

#[repr(C)]
pub struct PcapTimeval {
    pub tv_sec: i32,
    pub tv_usec: i32,
}

/// `struct pcap_pkthdr`. Windows' `timeval` uses 32-bit fields (LLP64), giving this a fixed,
/// platform-independent-of-bitness 16-byte layout; we don't read `ts` (see [`crate::capture`]
/// for why capture time comes from a monotonic clock instead).
#[repr(C)]
pub struct pcap_pkthdr {
    pub ts: PcapTimeval,
    pub caplen: u32,
    pub len: u32,
}

/// `struct bpf_program`. `bf_insns` is only ever handed back to libpcap, never dereferenced here.
#[repr(C)]
pub struct bpf_program {
    pub bf_len: u32,
    pub bf_insns: *mut c_void,
}

pub const PCAP_ERRBUF_SIZE: usize = 256;
pub const PCAP_NETMASK_UNKNOWN: u32 = 0xffff_ffff;

/// Datalink types ([tcpdump.org/linktypes](https://www.tcpdump.org/linktypes.html)) this crate
/// knows how to strip.
pub const DLT_NULL: c_int = 0;
pub const DLT_EN10MB: c_int = 1;
pub const DLT_RAW: c_int = 12;

pub type PcapFindAllDevsFn = unsafe extern "C" fn(*mut *mut pcap_if_t, *mut c_char) -> c_int;
pub type PcapFreeAllDevsFn = unsafe extern "C" fn(*mut pcap_if_t);
pub type PcapCreateFn = unsafe extern "C" fn(*const c_char, *mut c_char) -> *mut pcap_t;
pub type PcapSetSnaplenFn = unsafe extern "C" fn(*mut pcap_t, c_int) -> c_int;
pub type PcapSetPromiscFn = unsafe extern "C" fn(*mut pcap_t, c_int) -> c_int;
pub type PcapSetTimeoutFn = unsafe extern "C" fn(*mut pcap_t, c_int) -> c_int;
pub type PcapActivateFn = unsafe extern "C" fn(*mut pcap_t) -> c_int;
pub type PcapCompileFn =
    unsafe extern "C" fn(*mut pcap_t, *mut bpf_program, *const c_char, c_int, u32) -> c_int;
pub type PcapSetFilterFn = unsafe extern "C" fn(*mut pcap_t, *mut bpf_program) -> c_int;
pub type PcapFreeCodeFn = unsafe extern "C" fn(*mut bpf_program);
pub type PcapNextExFn =
    unsafe extern "C" fn(*mut pcap_t, *mut *mut pcap_pkthdr, *mut *const c_uchar) -> c_int;
pub type PcapDatalinkFn = unsafe extern "C" fn(*mut pcap_t) -> c_int;
pub type PcapCloseFn = unsafe extern "C" fn(*mut pcap_t);
pub type PcapGetErrFn = unsafe extern "C" fn(*mut pcap_t) -> *const c_char;
