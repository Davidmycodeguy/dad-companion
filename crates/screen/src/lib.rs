//! Screen capture, Windows OCR and window/cursor helpers for "hover values".
//!
//! Replaces the Python pipeline in `DnDTools-src/UI/src/models/` — `screen_grab.py`, `win_ocr.py`
//! together with `win_ocr.ps1`, and `game_window.py` — with a single in-process Rust crate: no
//! PowerShell worker process, no temp-file round trip for OCR, and no ctypes.
//!
//! # Coordinates are physical pixels
//!
//! Every coordinate and size this crate deals with — [`grab`]'s [`Region`], [`cursor_position`],
//! [`screen_size`] — is in **physical screen pixels**, not the logical/scaled pixels a
//! DPI-unaware process sees. This matches `hover_panel.py`'s `use_physical_pixels`: on a scaled
//! display (e.g. 150% on a 4K panel), an unaware thread gets cursor positions and a screen size
//! that don't line up with what GDI actually captures.
//!
//! The host process must be per-monitor DPI aware for this to hold. The Tauri app declares that
//! in its manifest, so it needs nothing further. Anything else that links this crate (tests, the
//! `timing` example) has no manifest and should call [`make_process_dpi_aware`] once at start-up.
//!
//! # Modules at a glance
//!
//! - [`grab`]: a GDI `BitBlt` of one screen region into a [`tooltip::Frame`].
//! - [`Ocr`]: Windows' built-in OCR (`Windows.Media.Ocr`) over a [`tooltip::Frame`], in memory.
//! - [`cursor_position`], [`screen_size`], [`game_has_focus`]: the remaining small window/cursor
//!   helpers `hover_panel.py` and `game_window.py` needed.

mod error;
mod grab;
mod ocr;
mod raii;
mod window;

pub use error::Error;
pub use grab::grab;
pub use ocr::Ocr;
pub use window::{cursor_position, game_has_focus, make_process_dpi_aware, screen_size};

// Re-exported so callers of `grab`/`Ocr` don't also need a direct `tooltip` dependency just to
// name their types.
pub use tooltip::{Frame, OcrLine, Region};
