//! Ports `hotkeys.py`'s `HotkeyError` hierarchy.

use thiserror::Error;

/// Everything that can go wrong parsing or registering a global hotkey.
///
/// `hotkeys.py` modelled this as a class hierarchy (`HotkeyError` <- `HotkeyParseError`,
/// `HotkeyRegistrationError` <- `HotkeyConflictError`) so callers could catch broad or narrow.
/// Rust has no exception hierarchy to mirror, so this is a flat enum instead; callers that only
/// care about "was this a conflict" match [`HotkeyError::Conflict`] directly.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    /// The hotkey string could not be parsed. Ports `HotkeyParseError`.
    #[error("{0}")]
    Parse(String),

    /// The hotkey could not be registered with Windows. Ports `HotkeyRegistrationError`.
    #[error("{0}")]
    Registration(String),

    /// Two bindings would use the same hotkey. Ports `HotkeyConflictError`.
    #[error("{0}")]
    Conflict(String),
}
