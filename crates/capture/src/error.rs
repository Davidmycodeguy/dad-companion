//! Error types for the `capture` crate.

/// Everything that can go wrong while loading Npcap, choosing an adapter, or capturing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// `wpcap.dll` could not be found or loaded (Npcap is not installed, or is broken).
    #[error("Npcap does not appear to be installed (could not load wpcap.dll: {0}). Install Npcap from https://npcap.com/ and try again.")]
    NpcapNotFound(String),

    /// `wpcap.dll` loaded, but a required exported function was missing.
    #[error("wpcap.dll is missing the expected function `{0}`; this Npcap install may be too old")]
    MissingSymbol(&'static str),

    /// A libpcap call reported failure; `message` is `pcap_geterr`'s text when available.
    #[error("{operation} failed: {message}")]
    Pcap {
        operation: &'static str,
        message: String,
    },

    /// No suitable network adapter could be found automatically.
    #[error("no usable network adapter found (no default-route interface and no up, non-loopback adapter)")]
    NoAdapterFound,

    /// An adapter name was explicitly requested but does not exist.
    #[error("network adapter '{0}' was not found")]
    AdapterNotFound(String),

    /// `pcap_open`/`pcap_activate` failed in a way that looks like a permissions problem.
    #[error("opening the network adapter was denied ({0}); capturing packets on this Npcap install may require running as Administrator")]
    PermissionDenied(String),

    /// The background reader thread could not be started (an OS/resource failure).
    #[error("could not start the capture reader thread: {0}")]
    ThreadSpawnFailed(String),

    /// A BPF filter string built internally by this crate turned out to be invalid.
    #[error("invalid capture filter: {0}")]
    InvalidFilter(String),
}

impl Error {
    /// Builds a [`Error::Pcap`], unless `message` (typically `pcap_geterr`'s text) looks like an
    /// access/permission failure, in which case this returns [`Error::PermissionDenied`]
    /// instead — this is how Npcap reports "you need to run as Administrator".
    pub(crate) fn from_pcap_failure(operation: &'static str, message: String) -> Self {
        let lower = message.to_ascii_lowercase();
        if lower.contains("denied") || lower.contains("permission") || lower.contains("access") {
            Error::PermissionDenied(message)
        } else {
            Error::Pcap { operation, message }
        }
    }
}

/// Formats a raw Win32 error code the way Windows API failures are usually reported.
pub(crate) fn win32_error(context: &str, code: u32) -> String {
    format!("{context} (Win32 error {code})")
}
