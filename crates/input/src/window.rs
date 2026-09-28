//! Finding the game's window, its client area, resolution/window-mode, and bringing it to front.
//!
//! Ports `game_window.py`'s window lookup — **not** `game_has_focus`, which already lives in the
//! `screen` crate and is intentionally not duplicated here — plus the window/config parts of
//! `macros.py`: `get_window_area_pos`, `get_game_resolution`, `get_game_window_mode`.

use std::path::PathBuf;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, POINT, RECT};
pub use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, FindWindowW, GetClientRect, GetWindowThreadProcessId, SetForegroundWindow};

use crate::keyboard::{tap_alt, DEFAULT_TAP_ALT_DELAY};
use crate::sink::InputSink;

/// The game's window title, including the two trailing spaces the game itself uses. Ports
/// `get_window_area_pos`'s default `window_title` argument.
pub const GAME_WINDOW_TITLE: &str = "Dark and Darker  ";
/// The game's process image name, used as a fallback window lookup.
pub const GAME_PROCESS_EXE: &str = "DungeonCrawler.exe";
/// `FullscreenMode` value for windowed mode in `GameUserSettings.ini`. Ports `macros.py`'s `WINDOW_MODE`.
pub const WINDOWED_MODE: u32 = 2;

/// A window's client area, in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// Finds the game's top-level window: first by its exact title, falling back to the first shown
/// window owned by [`GAME_PROCESS_EXE`] if the title ever doesn't match. `None` if neither finds
/// anything — the game isn't running. Ports `get_window_area_pos`'s `FindWindow` call, extended
/// with the process-name fallback the task also asks for.
pub fn find_game_window() -> Option<HWND> {
    let title = to_wide_null(GAME_WINDOW_TITLE);
    // SAFETY: `title` is a valid null-terminated UTF-16 buffer, valid for the whole call.
    if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) } {
        if !hwnd.is_invalid() {
            return Some(hwnd);
        }
    }
    find_window_by_exe(GAME_PROCESS_EXE)
}

/// `hwnd`'s client area, in physical screen pixels, or `None` if either Win32 call fails (e.g. the
/// window closed in the meantime). Ports `get_window_area_pos`'s `ClientToScreen`/`GetClientRect`.
pub fn client_area(hwnd: HWND) -> Option<WindowRect> {
    let mut origin = POINT::default();
    // SAFETY: `hwnd` is caller-supplied; `origin` is a valid, writable `POINT` on the stack.
    if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
        return None;
    }
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is caller-supplied; `rect` is a valid, writable `RECT` on the stack.
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    Some(WindowRect { left: origin.x, top: origin.y, width: rect.right - rect.left, height: rect.bottom - rect.top })
}

/// Brings the game's window to the foreground.
///
/// Windows refuses an unsolicited `SetForegroundWindow` from a background process unless the
/// calling thread just generated real input, so this taps Alt first — the Python reference's own
/// [`tap_alt`] "clear stuck modifiers" trick doubles as exactly that workaround. Python has no
/// single function combining the two; this is this crate's synthesis of "the Python's focus
/// trick" the task asked for (see the final report).
pub fn bring_to_front(sink: &mut dyn InputSink, hwnd: HWND) -> windows::core::Result<()> {
    tap_alt(sink, DEFAULT_TAP_ALT_DELAY);
    // SAFETY: `hwnd` is caller-supplied; failure is reported via `GetLastError`, read below.
    if unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        Ok(())
    } else {
        Err(windows::core::Error::from_thread())
    }
}

/// Path to the game's own settings file, or `None` if `%LOCALAPPDATA%` isn't set. Ports the
/// hard-coded path shared by `get_game_resolution`/`get_game_window_mode`.
pub fn game_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("DungeonCrawler").join("Saved").join("Config").join("Windows").join("GameUserSettings.ini"))
}

/// The resolution recorded in `GameUserSettings.ini`'s content, or `None` if either key is
/// missing. Ports `get_game_resolution`, returning numbers directly instead of a `"WxH"` string
/// that existed only to be immediately re-split by its one caller.
pub fn resolution_from_ini(content: &str) -> Option<(u32, u32)> {
    Some((find_uint(content, "ResolutionSizeX=")?, find_uint(content, "ResolutionSizeY=")?))
}

/// The window mode recorded in `GameUserSettings.ini`'s content (compare against
/// [`WINDOWED_MODE`]), or `None` if the key is missing. Ports `get_game_window_mode`.
pub fn window_mode_from_ini(content: &str) -> Option<u32> {
    find_uint(content, "FullscreenMode=")
}

/// The first unsigned integer immediately following `key`'s first occurrence in `content`. Ports
/// the Python reference's `re.search(rf'{key}(\d+)', content)`.
fn find_uint(content: &str, key: &str) -> Option<u32> {
    let after = &content[content.find(key)? + key.len()..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Closes a process handle exactly once when dropped.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid handle owned exclusively by this guard.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct FindByExe<'a> {
    exe: &'a str,
    found: Option<HWND>,
}

/// `WNDENUMPROC` callback for [`find_window_by_exe`]: stops at the first window owned by the
/// target process.
unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // SAFETY: `lparam.0` is the address of a live `FindByExe` set by `find_window_by_exe`, which
    // does not return until `EnumWindows` — and therefore every call to this callback — does.
    let state = unsafe { &mut *(lparam.0 as *mut FindByExe) };
    if window_exe_name(hwnd).is_some_and(|name| name.eq_ignore_ascii_case(state.exe)) {
        state.found = Some(hwnd);
        return windows::core::BOOL(0); // stop enumerating
    }
    windows::core::BOOL(1) // keep looking
}

fn find_window_by_exe(exe: &str) -> Option<HWND> {
    let mut state = FindByExe { exe, found: None };
    let lparam = LPARAM(std::ptr::addr_of_mut!(state) as isize);
    // SAFETY: `enum_proc` runs only synchronously within this call, and `lparam` points to
    // `state`, which is declared above and outlives the call.
    unsafe {
        let _ = EnumWindows(Some(enum_proc), lparam);
    }
    state.found
}

/// The file name (e.g. `"DungeonCrawler.exe"`) of `hwnd`'s owning process, or `None` if any step
/// fails. Ported separately from `screen`'s equivalent (used there for `game_has_focus`) because
/// it is a private helper there, not exported — see this crate's final report.
fn window_exe_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: `hwnd` came from `EnumWindows`, so it is a live window handle; `pid` is a valid,
    // writable `u32` on the stack.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: `PROCESS_QUERY_LIMITED_INFORMATION` only allows reading basic info (enough for the
    // image name below); the handle is closed exactly once via `OwnedHandle`.
    let process = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    // SAFETY: `buffer` is valid for `len` `u16` elements; the call writes at most that many and
    // updates `len` to the number actually written.
    unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len) }.ok()?;
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Some(std::path::Path::new(&path).file_name()?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_resolution_and_window_mode_from_ini_content() {
        let ini = "[/Script/DungeonCrawler.DCGameUserSettings]\r\nResolutionSizeX=3840\r\nResolutionSizeY=2160\r\nFullscreenMode=2\r\n";
        assert_eq!(resolution_from_ini(ini), Some((3840, 2160)));
        assert_eq!(window_mode_from_ini(ini), Some(WINDOWED_MODE));
    }

    #[test]
    fn missing_keys_yield_none() {
        assert_eq!(resolution_from_ini("ResolutionSizeX=1920"), None);
        assert_eq!(window_mode_from_ini(""), None);
    }

    /// Read-only window enumeration; sends no input and is safe to run while the game is up.
    #[test]
    fn find_game_window_does_not_panic() {
        let _ = find_game_window();
    }
}
