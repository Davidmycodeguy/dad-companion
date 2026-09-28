//! Global hotkeys: parsing/canonicalizing/display (pure), and a Windows `RegisterHotKey` backend.
//!
//! Ports `hotkeys.py`. The cross-platform `keyboard`-package fallback backend is not ported: this
//! companion only ever runs on Windows, so only `_WindowsHotkeyBackend` has a Rust counterpart.

mod error;
mod manager;
mod parse;

pub use error::HotkeyError;
pub use manager::HotkeyManager;
pub use parse::{canonicalize_hotkey, format_hotkey_display, parse_hotkey, Modifier, ParsedHotkey};
