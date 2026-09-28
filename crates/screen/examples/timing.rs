//! Timing for the hot paths: a GDI grab of a tooltip-sized strip, a larger grab, `Ocr::new`, and
//! OCR of a realistic tooltip-sized image (first call, then the average of a few more).
//!
//! Run in release mode: `cargo run -p screen --release --example timing`.

use std::time::{Duration, Instant};

use screen::{grab, make_process_dpi_aware, Frame, Ocr, Region};
use windows::core::w;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, TextOutW, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DEFAULT_QUALITY, DIB_RGB_COLORS,
    FF_DONTCARE, FW_NORMAL, OUT_DEFAULT_PRECIS, TRANSPARENT,
};

/// How many extra OCR reads to average after the (slower) first one.
const WARMUP_OCR_RUNS: u32 = 10;

fn main() {
    let _ = make_process_dpi_aware();

    time_grab("grab 64x2100 strip", (0, 0, 64, 2100));
    time_grab("grab 900x1200", (0, 0, 900, 1200));

    let ocr_new_started = Instant::now();
    let ocr = match Ocr::new() {
        Ok(ocr) => ocr,
        Err(err) => {
            println!("Ocr::new: failed: {err} (no OCR language installed?) -- skipping OCR timing");
            return;
        }
    };
    println!("Ocr::new: {:?} (language: {})", ocr_new_started.elapsed(), ocr.language());

    let frame = render_tooltip_like_image();
    println!("OCR image: {}x{}", frame.width(), frame.height());

    let first_started = Instant::now();
    let first_lines = ocr.read(&frame).expect("OCR of the timing image should succeed");
    println!("OCR first call: {:?} ({} lines)", first_started.elapsed(), first_lines.len());

    let mut total = Duration::ZERO;
    for _ in 0..WARMUP_OCR_RUNS {
        let started = Instant::now();
        let _ = ocr.read(&frame).expect("OCR of the timing image should succeed");
        total += started.elapsed();
    }
    println!("OCR average of {WARMUP_OCR_RUNS} more calls: {:?}", total / WARMUP_OCR_RUNS);
}

/// Times a single `grab` and prints the result.
fn time_grab(label: &str, region: Region) {
    let started = Instant::now();
    match grab(region) {
        Ok(frame) => println!("{label}: {:?} ({}x{})", started.elapsed(), frame.width(), frame.height()),
        Err(err) => println!("{label}: failed after {:?}: {err}", started.elapsed()),
    }
}

/// A synthetic ~600x800 tooltip-like image: several lines of representative text on white,
/// independent of the real screen, so this timing is reproducible run to run.
fn render_tooltip_like_image() -> Frame {
    const WIDTH: usize = 600;
    const LINE_HEIGHT: usize = 64;
    const LINES: [&str; 12] = [
        "Longsword of the Bear",
        "Rarity: Epic",
        "+2 Strength",
        "+15% Attack Speed",
        "Armor Rating 45",
        "Weight: 4.5 kg",
        "Durability 120/120",
        "Requires Level 12",
        "Two-Handed Slashing",
        "Value: 3,200 gold",
        "Soulbound",
        "\"Forged in the old world.\"",
    ];
    render_lines(WIDTH, LINE_HEIGHT, &LINES)
}

/// Renders `lines` as black text on white, one per `line_height`-pixel row, into an RGB [`Frame`].
/// A trimmed-down copy of `tests/ocr_tests.rs`'s helper of the same shape (see there for the
/// fuller per-step reasoning behind each `// SAFETY:` note below).
fn render_lines(width: usize, line_height: usize, lines: &[&str]) -> Frame {
    let height = line_height * lines.len();
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
        .expect("CreateDIBSection should succeed for a small offscreen bitmap");
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
            40,
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
        SetTextColor(mem_dc, COLORREF(0));
        for (row, text) in lines.iter().enumerate() {
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let y = (row * line_height) as i32 + line_height as i32 / 4;
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
