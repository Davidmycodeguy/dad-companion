//! Windows mouse/keyboard input, global hotkeys and game-screen layout for clicking through Dark
//! and Darker's own UI with the real cursor.
//!
//! Ports five modules from the Python reference (`DnDTools-src/UI/src/models/`): `macros.py`
//! (SendInput mouse/keyboard, smooth moves, the stash drag), `hotkeys.py` (global hotkeys),
//! `point.py` (`Point`), `marketplace_layout.py` and the screen-position half of `macros.py`
//! (per-resolution layout, scaling, calibration).
//!
//! # Coordinates are physical pixels
//!
//! Every coordinate this crate deals with is a **physical screen pixel** in **virtual-desktop**
//! space (i.e. it can be negative, or beyond the primary monitor, on a multi-monitor setup). The
//! host process must be per-monitor DPI aware for that to be meaningful; the Tauri app declares
//! this in its manifest. This crate never calls `SetProcessDpiAwarenessContext` itself — see the
//! `screen` crate for that.
//!
//! # No real input from tests
//! [`InputSink`] is a trait so tests can assert on the sequence of moves/clicks/keys a macro
//! *would* send without ever calling `SendInput` for real ([`WindowsInput`] is the only
//! implementation that touches hardware).
//!
//! This module list grows as the crate is filled in; every module declared here already has a
//! file, so the crate always compiles.

mod actions;
mod cancel;
mod drag;
mod error;
pub mod hotkeys;
mod jitter;
mod keyboard;
mod layout;
mod mouse;
mod point;
mod round;
mod sink;
mod window;

pub use actions::{click_point, click_stash_tab};
pub use cancel::Cancel;
pub use drag::{cell_centre, move_from_to_reliable, DragEndpoint, DragSlot, DragState};
pub use error::Error;
pub use keyboard::{release_modifiers, send_key, tap_alt, DEFAULT_TAP_ALT_DELAY, VK_CONTROL, VK_MENU};
pub use layout::{
    apply_calibration, apply_window_offset, is_ultrawide, positions_for_resolution,
    stash_type_name, scale_for, scale_length, scale_point, tab_index_for_stash_type,
    CalibratedResolution, CalibrationOverride, Delta, RawPoint, Scale, ScreenLayout,
    BASE_RESOLUTION, DEFAULT_STASH_TAB_MAPPING, STANDARD_ASPECT, STASH_TAB_COUNT,
};
pub use layout::marketplace;
pub use mouse::{move_mouse_smooth, nudge_cursor, SmoothMoveOptions, DEFAULT_NUDGE};
pub use point::Point;
pub use sink::{InputEvent, InputSink, WindowsInput};
pub use window::{
    bring_to_front, client_area, find_game_window, game_config_path, resolution_from_ini,
    window_mode_from_ini, WindowRect, GAME_PROCESS_EXE, GAME_WINDOW_TITLE, HWND, WINDOWED_MODE,
};
