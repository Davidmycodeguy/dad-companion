//! Cursor position, screen size, DPI awareness and "does the game have focus", porting
//! `game_window.py` and the physical-pixel handling described in `hover_panel.py`.

use std::path::Path;

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{E_ACCESSDENIED, HWND, LPARAM, POINT};
use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, DESKTOPHORZRES, DESKTOPVERTRES};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};

use crate::raii::{ProcessHandle, ScreenDc};
use crate::Error;

/// Longest process image path `QueryFullProcessImageNameW` may return; generous versus MAX_PATH.
const MAX_EXE_PATH: usize = 1024;

/// In-game overlays that can hold keyboard focus while the game keeps drawing beneath them.
/// Mirrors `game_window.py`'s `OVERLAY_TITLES` exactly (window titles, not exe names).
const OVERLAY_TITLES: [&str; 3] = ["Discord Overlay", "NVIDIA GeForce Overlay", "NVIDIA Overlay"];

/// The cursor's position in physical screen pixels, or `None` if Windows couldn't report one.
///
/// Requires the calling process to be per-monitor DPI aware (see the crate docs in `lib.rs`); the
/// Tauri host declares this in its manifest, and [`make_process_dpi_aware`] covers tests/examples.
pub fn cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    // SAFETY: `point` is a valid, writable `POINT` on the stack; `GetCursorPos` only writes to it.
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some((point.x, point.y))
}

/// The primary monitor's size in physical pixels, e.g. `(3840, 2160)` on a 4K display.
///
/// Reads `GetDeviceCaps` on the whole-screen DC rather than `GetSystemMetrics`, so the result is
/// correct even when the calling process is *not* DPI aware (an unaware process would otherwise
/// see a size scaled down by Windows' DPI virtualization). Returns `(0, 0)` in the (very rare)
/// case that `GetDC` itself fails, since this function has no `Result` to report that through.
pub fn screen_size() -> (i32, i32) {
    // SAFETY: `None` asks for the DC of the whole screen; `ScreenDc` releases it exactly once,
    // and `GetDeviceCaps` below only reads capability values from it.
    let dc = unsafe { GetDC(None) };
    if dc.is_invalid() {
        return (0, 0);
    }
    let dc = ScreenDc(dc);
    unsafe { (GetDeviceCaps(Some(dc.0), DESKTOPHORZRES), GetDeviceCaps(Some(dc.0), DESKTOPVERTRES)) }
}

/// Makes this process per-monitor-v2 DPI aware, so cursor positions, screen size and captured
/// coordinates are physical pixels rather than values scaled by Windows' DPI virtualization.
///
/// The Tauri host declares this in its manifest already; call this once at start-up in anything
/// that has no manifest, such as tests and `examples/timing`.
pub fn make_process_dpi_aware() -> Result<(), Error> {
    // SAFETY: sets a process-wide flag and touches no memory; safe to call at start-up, and safe
    // to call more than once (see the `E_ACCESSDENIED` handling below).
    match unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) } {
        Ok(()) => Ok(()),
        // Already declared via manifest, or set earlier by this same process (repeated calls in
        // tests): both leave the process at least as DPI aware as we asked for.
        Err(e) if e.code() == E_ACCESSDENIED => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// True when the game (a process named `game_exe`, case-insensitive) has focus, or an in-game
/// overlay (Discord, NVIDIA) holds focus while the game's own window is still shown (not
/// minimised) beneath it.
///
/// Ports `game_window.py`'s `game_has_focus`, which took the foreground window's title and a
/// caller-supplied "is the game's window shown" flag; this version resolves both itself from
/// `game_exe`, but the overlay check still compares window *titles*, exactly as the Python does.
pub fn game_has_focus(game_exe: &str) -> bool {
    // SAFETY: takes no arguments; a null result just means no window currently has focus (e.g.
    // the desktop itself, or a secure-desktop UAC prompt).
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        return false;
    }
    if exe_name_matches(foreground, game_exe) {
        return true;
    }
    OVERLAY_TITLES.contains(&window_title(foreground).as_str()) && game_window_is_shown(game_exe)
}

/// True when `hwnd`'s owning process is named `exe` (case-insensitive).
fn exe_name_matches(hwnd: HWND, exe: &str) -> bool {
    window_exe_name(hwnd).is_some_and(|name| name.eq_ignore_ascii_case(exe))
}

/// `hwnd`'s window title, or an empty string if it has none or the calls fail.
fn window_title(hwnd: HWND) -> String {
    // SAFETY: `hwnd` is a live window handle (from `GetForegroundWindow` or `EnumWindows`).
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; len as usize + 1];
    // SAFETY: `buffer` has room for `len` characters plus the null terminator `GetWindowTextW`
    // writes in the worst case.
    let copied = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied as usize])
}

/// The file name (e.g. `"Game.exe"`) of `hwnd`'s owning process, or `None` if any step fails
/// (the window closed, the process exited, or it is protected against `PROCESS_QUERY_LIMITED_INFORMATION`).
fn window_exe_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: `hwnd` is a live window handle; `pid` is a valid, writable `u32` on the stack.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: `PROCESS_QUERY_LIMITED_INFORMATION` only allows reading basic info (enough for the
    // image name below); `process` is closed exactly once via `ProcessHandle`.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let process = ProcessHandle(process);
    let mut buffer = [0u16; MAX_EXE_PATH];
    let mut len = buffer.len() as u32;
    // SAFETY: `buffer` is valid for `len` `u16` elements; the call writes at most that many plus
    // updates `len` to the number actually written.
    unsafe {
        QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len)
    }
    .ok()?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Some(Path::new(&path).file_name()?.to_string_lossy().into_owned())
}

/// True when a window belonging to a process named `exe` (case-insensitive) exists and is shown
/// (visible, not minimised) somewhere, regardless of which window currently has focus.
fn game_window_is_shown(exe: &str) -> bool {
    let mut state = FindWindowByExe { exe, found: false };
    let lparam = LPARAM(std::ptr::addr_of_mut!(state) as isize);
    // SAFETY: `enum_proc` runs only synchronously within this `EnumWindows` call, and `lparam`
    // points to `state`, which is declared above and outlives the call.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), lparam);
    }
    state.found
}

/// Search state threaded through `EnumWindows` via its `LPARAM`.
struct FindWindowByExe<'a> {
    exe: &'a str,
    found: bool,
}

/// `WNDENUMPROC` callback for [`game_window_is_shown`]: stops (returns `FALSE`) at the first
/// window owned by the target process that is visible and not minimised.
unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam.0` is the address of a live `FindWindowByExe` set by `game_window_is_shown`,
    // which does not return until `EnumWindows` — and therefore every call to this callback —
    // does.
    let state = unsafe { &mut *(lparam.0 as *mut FindWindowByExe) };
    if !exe_name_matches(hwnd, state.exe) {
        return BOOL(1); // keep enumerating
    }
    // SAFETY: `hwnd` was just supplied by `EnumWindows`, so it is a live window handle.
    let shown = unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
    state.found = shown;
    if shown {
        BOOL(0) // stop: we found a shown window
    } else {
        BOOL(1) // keep looking; the game may have more than one top-level window
    }
}
