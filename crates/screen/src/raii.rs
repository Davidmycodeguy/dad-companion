//! RAII guards that release GDI/Win32 handles on every return path (including `?`), the same
//! handles `screen_grab.py`'s `try`/`finally` and `game_window.py`'s callers release by hand.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, ReleaseDC, HBITMAP, HDC};

/// A DC from `GetDC(None)` (the whole screen); released with `ReleaseDC` when dropped.
pub(crate) struct ScreenDc(pub HDC);

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: `self.0` was returned by a prior `GetDC(None)` and each `ScreenDc` releases it
        // exactly once, since `Drop::drop` runs at most once per value.
        unsafe {
            ReleaseDC(None, self.0);
        }
    }
}

/// A DC from `CreateCompatibleDC`; deleted with `DeleteDC` when dropped.
pub(crate) struct MemDc(pub HDC);

impl Drop for MemDc {
    fn drop(&mut self) {
        // SAFETY: `self.0` was returned by a prior `CreateCompatibleDC` and each `MemDc` deletes
        // it exactly once.
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

/// A GDI bitmap (here, always a DIB section from `CreateDIBSection`); deleted when dropped.
pub(crate) struct GdiBitmap(pub HBITMAP);

impl Drop for GdiBitmap {
    fn drop(&mut self) {
        // SAFETY: `self.0` was returned by a prior `CreateDIBSection` and each `GdiBitmap`
        // deletes it exactly once, after it has been deselected from its DC.
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

/// A process handle from `OpenProcess`; closed with `CloseHandle` when dropped.
pub(crate) struct ProcessHandle(pub HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: `self.0` was returned by a prior `OpenProcess` and each `ProcessHandle` closes
        // it exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
