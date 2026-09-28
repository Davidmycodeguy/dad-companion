//! GDI screen capture: a `BitBlt` of just the requested region, like `screen_grab.py`.

use tooltip::{Frame, Region};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, GetDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};

use crate::raii::{GdiBitmap, MemDc, ScreenDc};
use crate::Error;

/// Bytes per pixel in the 32-bit BGRA DIB `grab` renders into.
const BGRA_BYTES_PER_PIXEL: usize = 4;

/// A GDI capture of just `region`'s pixels, converted from BGRA to RGB.
///
/// Ports `screen_grab.py`'s `grab`: a `BitBlt` of only the requested rectangle (via a top-down
/// 32bpp `CreateDIBSection`) costs about the region's own size instead of the whole desktop's,
/// which is what makes this fast enough for a per-hover capture (see that file's module doc).
///
/// `region` is `(left, top, right, bottom)` in physical screen pixels, right/bottom exclusive.
/// A region with non-positive width or height (empty or inverted) returns an empty (0x0) frame
/// without touching the screen, matching the Python version's behaviour for a degenerate region.
pub fn grab(region: Region) -> Result<Frame, Error> {
    let (left, top, right, bottom) = region;
    let (width, height) = (right - left, bottom - top);
    if width <= 0 || height <= 0 {
        return Ok(Frame::new(0, 0, Vec::new()));
    }
    let (width, height) = (width as usize, height as usize);

    // SAFETY: `None` asks for the DC of the whole screen (`user32.GetDC(None)` in
    // screen_grab.py); `ScreenDc` releases it exactly once on every return path below.
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.is_invalid() {
        return Err(windows::core::Error::from_thread().into());
    }
    let screen_dc = ScreenDc(screen_dc);

    // SAFETY: `screen_dc.0` is the valid DC obtained above and outlives this call.
    let mem_dc = unsafe { CreateCompatibleDC(Some(screen_dc.0)) };
    if mem_dc.is_invalid() {
        return Err(windows::core::Error::from_thread().into());
    }
    let mem_dc = MemDc(mem_dc);

    let bitmap_info =
        BITMAPINFO { bmiHeader: top_down_32bpp_header(width, height), ..Default::default() };
    let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
    // SAFETY: `bitmap_info` describes a top-down 32bpp DIB of `width` x `height` pixels;
    // `CreateDIBSection` allocates a buffer matching that description and writes its address
    // into `bits`, valid until the returned bitmap is deleted.
    let bitmap =
        unsafe { CreateDIBSection(Some(mem_dc.0), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
    let bitmap = GdiBitmap(bitmap);

    // SAFETY: selects the DIB section into the memory DC so `BitBlt` draws into our buffer.
    let previous = unsafe { SelectObject(mem_dc.0, bitmap.0.into()) };
    // SAFETY: copies `width` x `height` pixels from the screen DC at `(left, top)` into the DIB;
    // both DCs are the valid, live handles obtained above.
    let copied = unsafe {
        BitBlt(mem_dc.0, 0, 0, width as i32, height as i32, Some(screen_dc.0), left, top, SRCCOPY)
    };
    // SAFETY: restores the memory DC's previous bitmap before `bitmap` is deleted below, as
    // screen_grab.py does before its own cleanup.
    unsafe {
        SelectObject(mem_dc.0, previous);
    }
    copied?;

    // `CreateDIBSection` returning `Ok` guarantees `bits` was written (it only stays null on
    // failure, already handled above via `?`); asserted here as a cheap, explicit belt-and-suspenders
    // check on that contract before trusting the pointer in the unsafe block below.
    debug_assert!(!bits.is_null(), "CreateDIBSection succeeded but left `bits` null");

    // SAFETY: `bits` points to `width * height * BGRA_BYTES_PER_PIXEL` initialized bytes that
    // `bitmap` owns and that `BitBlt` above finished writing; `bitmap` is still alive (its guard
    // has not been dropped yet), and `Frame::from_bgra` copies the bytes into a new `Vec` before
    // this function returns and `bitmap`'s `Drop` frees the underlying buffer.
    let pixels = unsafe {
        std::slice::from_raw_parts(bits as *const u8, width * height * BGRA_BYTES_PER_PIXEL)
    };
    Ok(Frame::from_bgra(width, height, pixels))
}

/// A `BITMAPINFOHEADER` for a top-down (negative height) 32bpp DIB, matching
/// `screen_grab.py`'s `top_down_32bpp`: row 0 is the top row, so no vertical flip is needed.
fn top_down_32bpp_header(width: usize, height: usize) -> BITMAPINFOHEADER {
    const BITS_PER_PIXEL: u16 = 32;
    const COLOR_PLANES: u16 = 1;
    BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: COLOR_PLANES,
        biBitCount: BITS_PER_PIXEL,
        biCompression: BI_RGB.0,
        ..Default::default()
    }
}
