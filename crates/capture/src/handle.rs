//! Safe, RAII wrapper around one open libpcap capture session (`pcap_t*`).

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::Arc;

use crate::error::Error;
use crate::ffi::{bpf_program, pcap_pkthdr, pcap_t, PCAP_ERRBUF_SIZE, PCAP_NETMASK_UNKNOWN};
use crate::library::NpcapLibrary;

/// How long `pcap_next_ex` blocks waiting for a packet before reporting "timed out". Short
/// enough that `stop()` (which just sets a flag this crate's reader thread checks between
/// reads) returns promptly.
pub const READ_TIMEOUT_MS: c_int = 200;

/// Bytes captured per packet: comfortably above the largest possible Ethernet+IP+TCP frame, so
/// packets are never truncated.
const SNAPLEN: c_int = 65535;

/// One open, activated, filtered capture session. Closes itself on drop.
pub struct PcapHandle {
    lib: Arc<NpcapLibrary>,
    handle: *mut pcap_t,
}

// SAFETY: `PcapHandle` exclusively owns its `pcap_t*` — it is never copied or exposed — and
// every method that touches `self.handle` takes `&mut self`, so within this process at most one
// thread can be calling into libpcap through a given handle at a time, matching libpcap's
// documented thread-safety contract for live captures.
unsafe impl Send for PcapHandle {}

impl PcapHandle {
    /// Opens `device_name`, compiles and installs `bpf_filter`, and activates the session.
    pub fn open(lib: Arc<NpcapLibrary>, device_name: &str, bpf_filter: &str) -> Result<Self, Error> {
        let mut errbuf = [0 as c_char; PCAP_ERRBUF_SIZE];
        let c_name = CString::new(device_name)
            .map_err(|_| Error::AdapterNotFound(device_name.to_string()))?;

        // SAFETY: `c_name` is NUL-terminated and outlives this call; `errbuf` is a valid,
        // correctly-sized out-parameter for `pcap_create` to write an error message into.
        let handle = unsafe { (lib.create)(c_name.as_ptr(), errbuf.as_mut_ptr()) };
        if handle.is_null() {
            return Err(Error::from_pcap_failure(
                "opening the adapter",
                c_buf_to_string(&errbuf),
            ));
        }

        let mut session = PcapHandle { lib, handle };
        session.activate()?;
        session.set_filter(bpf_filter)?;
        Ok(session)
    }

    fn activate(&mut self) -> Result<(), Error> {
        // SAFETY: `self.handle` was just returned by a successful `pcap_create` and has not
        // been activated yet, which is exactly when libpcap allows calling these setters.
        unsafe {
            (self.lib.set_snaplen)(self.handle, SNAPLEN);
            // Promiscuous mode is left off: the game client's own traffic to/from this PC is
            // delivered to it regardless, so there is no need to see other hosts' frames.
            (self.lib.set_promisc)(self.handle, 0);
            (self.lib.set_timeout)(self.handle, READ_TIMEOUT_MS);
        }

        // SAFETY: `self.handle` is live; the return code is checked immediately below.
        let rc = unsafe { (self.lib.activate)(self.handle) };
        if rc < 0 {
            return Err(Error::from_pcap_failure(
                "pcap_activate",
                self.last_error("pcap_activate"),
            ));
        }
        Ok(())
    }

    fn set_filter(&mut self, bpf_filter: &str) -> Result<(), Error> {
        let c_filter = CString::new(bpf_filter).map_err(|_| {
            Error::InvalidFilter(format!("'{bpf_filter}' contains an embedded NUL byte"))
        })?;
        let mut program = bpf_program {
            bf_len: 0,
            bf_insns: ptr::null_mut(),
        };

        // SAFETY: `self.handle` is live and activated; `c_filter` is NUL-terminated and outlives
        // the call; `program` is a valid, zeroed out-parameter for `pcap_compile` to fill in.
        let rc = unsafe {
            (self.lib.compile)(
                self.handle,
                &mut program,
                c_filter.as_ptr(),
                1,
                PCAP_NETMASK_UNKNOWN,
            )
        };
        if rc < 0 {
            return Err(Error::from_pcap_failure(
                "pcap_compile",
                self.last_error("pcap_compile"),
            ));
        }

        // SAFETY: `program` was just successfully filled in by `pcap_compile` above, and
        // `self.handle` is live.
        let rc = unsafe { (self.lib.set_filter)(self.handle, &mut program) };
        // SAFETY: `program` was successfully compiled above; freed exactly once, after
        // `pcap_setfilter` has copied whatever it needs from it.
        unsafe { (self.lib.free_code)(&mut program) };

        if rc < 0 {
            return Err(Error::from_pcap_failure(
                "pcap_setfilter",
                self.last_error("pcap_setfilter"),
            ));
        }
        Ok(())
    }

    /// The link-layer header type this adapter delivers (e.g. Ethernet, Npcap loopback).
    pub fn datalink(&self) -> c_int {
        // SAFETY: `self.handle` is a live, activated handle for the lifetime of `self`.
        unsafe { (self.lib.datalink)(self.handle) }
    }

    /// Reads the next packet, waiting up to [`READ_TIMEOUT_MS`].
    ///
    /// Returns `Ok(None)` on a read timeout (no packet arrived) — the normal, frequent case that
    /// lets a reader loop poll a stop flag without blocking indefinitely.
    pub fn next_packet(&mut self) -> Result<Option<(&pcap_pkthdr, &[u8])>, Error> {
        let mut header: *mut pcap_pkthdr = ptr::null_mut();
        let mut data: *const u8 = ptr::null();

        // SAFETY: `self.handle` is live; `header`/`data` are valid out-parameters that
        // `pcap_next_ex` will, on success, point at libpcap-owned buffers valid until the next
        // call made on this same handle (which is why the returned slice borrows `self`).
        let rc = unsafe { (self.lib.next_ex)(self.handle, &mut header, &mut data) };
        match rc {
            1 => {
                // SAFETY: `rc == 1` means libpcap populated both out-parameters; `caplen` is
                // libpcap's own bound on how many bytes of `data` are valid to read.
                let header_ref = unsafe { &*header };
                let slice =
                    unsafe { std::slice::from_raw_parts(data, header_ref.caplen as usize) };
                Ok(Some((header_ref, slice)))
            }
            0 => Ok(None),
            _ => Err(Error::from_pcap_failure(
                "pcap_next_ex",
                self.last_error("pcap_next_ex"),
            )),
        }
    }

    fn last_error(&self, operation: &'static str) -> String {
        // SAFETY: `self.handle` is live; `pcap_geterr` always returns a pointer to a
        // NUL-terminated string owned by the handle (empty if nothing went wrong yet).
        let message = unsafe { CStr::from_ptr((self.lib.get_err)(self.handle)) }
            .to_string_lossy()
            .into_owned();
        if message.is_empty() {
            operation.to_string()
        } else {
            message
        }
    }
}

impl Drop for PcapHandle {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is only ever closed here, exactly once (ordinary `Drop`), and no
        // other code holds a copy of the raw pointer.
        unsafe { (self.lib.close)(self.handle) }
    }
}

fn c_buf_to_string(buf: &[c_char; PCAP_ERRBUF_SIZE]) -> String {
    // SAFETY: `buf` is a local, zero-initialized buffer that libpcap NUL-terminates on error, so
    // it is always a valid, in-bounds C string.
    unsafe { CStr::from_ptr(buf.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
