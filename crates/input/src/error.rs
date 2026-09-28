//! The crate's single error type.

use thiserror::Error;

/// Everything a macro-style input operation can fail with.
///
/// Ports `macros.py`'s `MacroCancelled` (see [`Error::Cancelled`]) and `hotkeys.py`'s
/// `HotkeyError` hierarchy (see [`Error::Hotkey`]). Win32 failures are wrapped rather than
/// converted to a string up front, so callers that care can still inspect the HRESULT.
#[derive(Debug, Error)]
pub enum Error {
    /// A cancel token was set while a macro was sleeping or between steps. Ports `MacroCancelled`.
    #[error("macro was cancelled")]
    Cancelled,

    /// A global-hotkey operation failed: parsing, registration or a conflicting binding.
    #[error(transparent)]
    Hotkey(#[from] crate::hotkeys::HotkeyError),

    /// A Win32 call failed.
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
}
