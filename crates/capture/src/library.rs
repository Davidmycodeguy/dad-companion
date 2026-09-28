//! Loads `wpcap.dll` at runtime (no Npcap SDK / `.lib` needed) and resolves the handful of
//! libpcap functions this crate calls.

use std::ffi::{c_void, CStr, OsStr};
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use crate::error::{win32_error, Error};
use crate::ffi::*;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(lp_lib_file_name: *const u16, h_file: *mut c_void, dw_flags: u32) -> *mut c_void;
    fn FreeLibrary(h_lib_module: *mut c_void) -> i32;
    fn GetProcAddress(h_module: *mut c_void, lp_proc_name: *const i8) -> *mut c_void;
    fn GetLastError() -> u32;
}

/// Makes `LoadLibraryExW` search the directory containing `wpcap.dll` first when resolving
/// *its* dependency on `Packet.dll`. Npcap installs both side by side outside the default DLL
/// search path (e.g. `System32\Npcap\`), so without this flag a plain `LoadLibrary` on the full
/// path to `wpcap.dll` loads but then fails the moment it tries to import `Packet.dll`.
const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;

/// A loaded `wpcap.dll`, with every exported function this crate uses resolved up front.
pub struct NpcapLibrary {
    module: *mut c_void,
    pub find_all_devs: PcapFindAllDevsFn,
    pub free_all_devs: PcapFreeAllDevsFn,
    pub create: PcapCreateFn,
    pub set_snaplen: PcapSetSnaplenFn,
    pub set_promisc: PcapSetPromiscFn,
    pub set_timeout: PcapSetTimeoutFn,
    pub activate: PcapActivateFn,
    pub compile: PcapCompileFn,
    pub set_filter: PcapSetFilterFn,
    pub free_code: PcapFreeCodeFn,
    pub next_ex: PcapNextExFn,
    pub datalink: PcapDatalinkFn,
    pub close: PcapCloseFn,
    pub get_err: PcapGetErrFn,
}

// SAFETY: `NpcapLibrary` only holds a module handle and plain C function pointers, with no
// interior mutability; libpcap's live-capture functions are documented as callable from any
// thread as long as a given `pcap_t*` is only used by one thread at a time (upheld by
// `PcapHandle` owning its handle exclusively), so sharing the *library* itself across threads
// (e.g. behind an `Arc`) is sound.
unsafe impl Send for NpcapLibrary {}
unsafe impl Sync for NpcapLibrary {}

impl NpcapLibrary {
    /// Loads `wpcap.dll` and resolves every function this crate needs, or fails with a clear
    /// "install Npcap" error.
    pub fn load() -> Result<Self, Error> {
        let module = load_wpcap_module()?;
        // SAFETY: `module` was just returned by a successful `LoadLibraryExW`, so it is a
        // valid, currently-loaded module handle for every `resolve` call below.
        unsafe {
            Ok(NpcapLibrary {
                module,
                find_all_devs: resolve(module, c"pcap_findalldevs")?,
                free_all_devs: resolve(module, c"pcap_freealldevs")?,
                create: resolve(module, c"pcap_create")?,
                set_snaplen: resolve(module, c"pcap_set_snaplen")?,
                set_promisc: resolve(module, c"pcap_set_promisc")?,
                set_timeout: resolve(module, c"pcap_set_timeout")?,
                activate: resolve(module, c"pcap_activate")?,
                compile: resolve(module, c"pcap_compile")?,
                set_filter: resolve(module, c"pcap_setfilter")?,
                free_code: resolve(module, c"pcap_freecode")?,
                next_ex: resolve(module, c"pcap_next_ex")?,
                datalink: resolve(module, c"pcap_datalink")?,
                close: resolve(module, c"pcap_close")?,
                get_err: resolve(module, c"pcap_geterr")?,
            })
        }
    }
}

impl Drop for NpcapLibrary {
    fn drop(&mut self) {
        // SAFETY: `self.module` was returned by a successful `LoadLibraryExW` in `load` and is
        // freed at most once (ordinary `Drop`). Every `PcapHandle` keeps this library alive
        // (via `Arc`) for as long as it might still call through these function pointers, so
        // none are in use once we get here.
        unsafe {
            FreeLibrary(self.module);
        }
    }
}

/// Resolves one exported function by name.
///
/// # Safety
/// `module` must be a valid, currently-loaded module handle, and `T` must be a function-pointer
/// type whose signature exactly matches the real C function named by `name`.
unsafe fn resolve<T>(module: *mut c_void, name: &'static CStr) -> Result<T, Error> {
    // A generic `T` has no size known to the type system, so `transmute` (which requires
    // statically-equal sizes) can't be used here; every real instantiation is a function
    // pointer, which this assertion confirms is pointer-sized like `addr`.
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut c_void>());

    // SAFETY: `module` is valid per this function's contract; `name` is a NUL-terminated
    // `'static` C string, satisfying `GetProcAddress`'s contract for `lpProcName`.
    let addr = unsafe { GetProcAddress(module, name.as_ptr()) };
    if addr.is_null() {
        return Err(Error::MissingSymbol(
            name.to_str().unwrap_or("<non-UTF-8 symbol name>"),
        ));
    }
    // SAFETY: `addr` is a non-null address `GetProcAddress` resolved for `name`; per this
    // function's contract, `T` is the correct function-pointer type for that symbol. Both `T`
    // and `*mut c_void` are pointer-sized (checked above), so copying `addr`'s bytes into a `T`
    // is a valid reinterpretation.
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&addr) })
}

/// Finds and loads `wpcap.dll`: Npcap's own install directory first, then wherever the default
/// search order would find it (classic WinPcap, or Npcap installed in "WinPcap API-compatible
/// Mode", which also copies it straight into `System32`).
fn load_wpcap_module() -> Result<*mut c_void, Error> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    let candidates = [
        format!(r"{system_root}\System32\Npcap\wpcap.dll"),
        format!(r"{system_root}\System32\wpcap.dll"),
        "wpcap.dll".to_string(),
    ];

    let mut last_error = String::new();
    for path in &candidates {
        match try_load(path) {
            Ok(module) => return Ok(module),
            Err(message) => last_error = message,
        }
    }
    Err(Error::NpcapNotFound(last_error))
}

fn try_load(path: &str) -> Result<*mut c_void, String> {
    let wide: Vec<u16> = OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives this call; `h_file` is
    // null as `LoadLibraryExW` requires.
    let module =
        unsafe { LoadLibraryExW(wide.as_ptr(), ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
    if module.is_null() {
        // SAFETY: `GetLastError` takes no arguments and only reads thread-local state.
        let code = unsafe { GetLastError() };
        Err(win32_error(&format!("could not load '{path}'"), code))
    } else {
        Ok(module)
    }
}
