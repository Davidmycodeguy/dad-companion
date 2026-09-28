//! End-to-end OCR test: render synthetic text into an offscreen DIB (independent of whatever is
//! actually on screen, so there is known ground truth), then OCR it and check the recognised
//! lines: present, top to bottom, with sensible (non-degenerate) boxes.

use screen::{Frame, Ocr};
use windows::core::w;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, TextOutW, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DIB_RGB_COLORS,
    FF_DONTCARE, FW_NORMAL, OUT_DEFAULT_PRECIS, TRANSPARENT,
};

/// Row height for [`render_lines`]; generous next to [`FONT_HEIGHT`] so lines never touch.
const LINE_HEIGHT: usize = 64;
/// The rendered font's character height in logical pixels: large and plain, so on-device OCR
/// reads it reliably.
const FONT_HEIGHT: i32 = 40;

#[test]
fn recognises_synthetic_lines_top_to_bottom() {
    let ocr = match Ocr::new() {
        Ok(ocr) => ocr,
        Err(err) => {
            println!("skipping OCR test: {err} (this machine likely has no OCR language installed)");
            return;
        }
    };

    let expected = ["Rarity: Epic", "+2 Strength", "Armor Rating 45"];
    let frame = render_lines(600, &expected);

    let lines = ocr.read(&frame).expect("OCR of a clean synthetic image should succeed");
    assert!(
        lines.len() >= expected.len(),
        "expected at least {} recognised lines, got {lines:?}",
        expected.len()
    );
    for pair in lines.windows(2) {
        assert!(pair[0].y <= pair[1].y, "lines are not top-to-bottom: {lines:?}");
    }
    for text in expected {
        let line = lines
            .iter()
            .find(|line| line.text.contains(text))
            .unwrap_or_else(|| panic!("expected a recognised line containing {text:?}, got {lines:?}"));
        assert!(line.w > 0 && line.h > 0, "line {text:?} has a degenerate box: {line:?}");
    }
}

/// Renders `lines` as black text on white, one per [`LINE_HEIGHT`]-pixel row, into an RGB
/// [`Frame`]. Independent of `screen::grab`: this draws into an offscreen DIB instead of reading
/// the real screen, so the test above has known ground truth.
fn render_lines(width: usize, lines: &[&str]) -> Frame {
    let height = LINE_HEIGHT * lines.len();

    // SAFETY: the same two calls `grab` makes for the real screen; both handles are released
    // below, exactly once, on this function's single (success) path.
    let (screen_dc, mem_dc) = unsafe {
        let screen_dc = GetDC(None);
        (screen_dc, CreateCompatibleDC(Some(screen_dc)))
    };

    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let bmi = BITMAPINFO { bmiHeader: header, ..Default::default() };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: `bmi` describes a top-down 32bpp DIB of `width` x `height` pixels, matching the
    // buffer `CreateDIBSection` allocates and points `bits` at.
    let bitmap = unsafe { CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }
        .expect("CreateDIBSection should succeed for a small test bitmap");

    // SAFETY: `bits` points to `width * height * 4` freshly allocated bytes owned by `bitmap`;
    // filling it white here, before selecting it into `mem_dc`, is a plain memory write.
    unsafe {
        std::ptr::write_bytes(bits as *mut u8, 0xFF, width * height * 4);
    }
    // SAFETY: `mem_dc` and `bitmap` are the valid handles obtained above.
    let previous_bitmap = unsafe { SelectObject(mem_dc, bitmap.into()) };

    // SAFETY: a stock UI font, no device-dependent resources beyond `mem_dc`; `font` is deleted
    // below once it is no longer selected into `mem_dc`.
    let font = unsafe {
        CreateFontW(
            FONT_HEIGHT,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            u32::from(DEFAULT_PITCH.0) | u32::from(FF_DONTCARE.0),
            w!("Segoe UI"),
        )
    };
    assert!(!font.is_invalid(), "CreateFontW should succeed for a stock UI font");

    // SAFETY: `mem_dc` is the valid DC selected above; each `text` slice is a plain UTF-16 buffer
    // alive for the duration of its own `TextOutW` call.
    let previous_font = unsafe {
        let previous_font = SelectObject(mem_dc, font.into());
        SetBkMode(mem_dc, TRANSPARENT);
        SetTextColor(mem_dc, COLORREF(0)); // black
        for (row, text) in lines.iter().enumerate() {
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let y = (row * LINE_HEIGHT) as i32 + LINE_HEIGHT as i32 / 4;
            let _ = TextOutW(mem_dc, 8, y, &utf16);
        }
        previous_font
    };

    // SAFETY: `bits` still points to the `width * height * 4` live bytes `bitmap` owns, now with
    // the text drawn into them; `Frame::from_bgra` copies the bytes out before `bitmap` (and its
    // buffer) is deleted just below.
    let frame = unsafe {
        let pixels = std::slice::from_raw_parts(bits as *const u8, width * height * 4);
        Frame::from_bgra(width, height, pixels)
    };

    // SAFETY: releases every handle created above, exactly once, mirroring `grab`'s cleanup.
    unsafe {
        SelectObject(mem_dc, previous_font);
        let _ = DeleteObject(font.into());
        SelectObject(mem_dc, previous_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
    }

    frame
}
