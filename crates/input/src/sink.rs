//! `SendInput` behind a trait, so tests can assert on what a macro *would* send without ever
//! moving the real mouse (the owner may be mid-game when tests run).

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use crate::round::python_round;

/// One primitive `SendInput`/`GetCursorPos` action, recorded verbatim by test sinks so a macro's
/// behaviour can be asserted without touching real hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    MoveTo { x: i32, y: i32 },
    MouseDown,
    MouseUp,
    Key { vk: u16, key_up: bool },
}

/// Everything a macro needs from the mouse/keyboard, as a trait so tests can substitute a
/// recording fake for [`WindowsInput`]. Ports `macros.py`'s `move_mouse`/`mouse_down`/`mouse_up`/
/// `send_key`/`GetCursorPos` calls.
pub trait InputSink {
    /// Absolute move to `(x, y)` in physical virtual-desktop pixels. Ports `move_mouse`.
    fn move_to(&mut self, x: i32, y: i32);
    /// Ports `mouse_down`.
    fn mouse_down(&mut self);
    /// Ports `mouse_up`.
    fn mouse_up(&mut self);
    /// Ports `send_key(vk_code, key_up)`.
    fn key_event(&mut self, vk: u16, key_up: bool);
    /// The current cursor position, or `(0, 0)` if Windows can't report one. Ports the
    /// `GetCursorPos` call inlined in `nudge_cursor`/`move_from_to_reliable`, which likewise never
    /// checked the return value.
    fn cursor_position(&self) -> (i32, i32);
}

/// Sends real input via `SendInput`. The only [`InputSink`] that touches hardware.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsInput;

/// One `INPUT` carrying a single mouse event with the given flags/deltas.
fn mouse_input(dx: i32, dy: i32, flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    let mut input = INPUT { r#type: INPUT_MOUSE, ..Default::default() };
    input.Anonymous.mi = MOUSEINPUT { dx, dy, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 };
    input
}

/// Sends one `INPUT`, ignoring the returned "number of events accepted" count, exactly as the
/// Python reference ignores `SendInput`'s return value.
fn send_one(input: INPUT) {
    // SAFETY: `input` is one fully-initialized `INPUT` on the stack; `SendInput` reads it
    // synchronously and keeps no pointer into it afterward.
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
}

impl InputSink for WindowsInput {
    fn move_to(&mut self, x: i32, y: i32) {
        // SAFETY: no arguments; these read-only system metrics are always available.
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        // Absolute mouse coordinates are normalized to 0..=65535 over the addressed desktop area,
        // per `MOUSEEVENTF_VIRTUALDESK`'s docs. The Python reference normalized over
        // `GetSystemMetrics(SM_CXSCREEN/CYSCREEN)` (the primary monitor only) instead, which
        // placed the game window incorrectly whenever it wasn't on the primary monitor; this is a
        // deliberate improvement, not a straight port.
        let abs_x = python_round((x - vx) as f64 * 65535.0 / (vw - 1).max(1) as f64) as i32;
        let abs_y = python_round((y - vy) as f64 * 65535.0 / (vh - 1).max(1) as f64) as i32;
        send_one(mouse_input(abs_x, abs_y, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK));
    }

    fn mouse_down(&mut self) {
        send_one(mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN));
    }

    fn mouse_up(&mut self) {
        send_one(mouse_input(0, 0, MOUSEEVENTF_LEFTUP));
    }

    fn key_event(&mut self, vk: u16, key_up: bool) {
        let mut input = INPUT { r#type: INPUT_KEYBOARD, ..Default::default() };
        input.Anonymous.ki = KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            wScan: 0,
            dwFlags: if key_up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
            time: 0,
            dwExtraInfo: 0,
        };
        send_one(input);
    }

    fn cursor_position(&self) -> (i32, i32) {
        let mut point = POINT::default();
        // SAFETY: `point` is a valid, writable `POINT` on the stack; on failure it is left
        // zeroed, matching the Python reference (which never checked this call's return value).
        match unsafe { GetCursorPos(&mut point) } {
            Ok(()) => (point.x, point.y),
            Err(_) => (0, 0),
        }
    }
}
