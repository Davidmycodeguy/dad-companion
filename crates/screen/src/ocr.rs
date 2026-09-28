//! Windows' built-in OCR (`Windows.Media.Ocr`), ported from `win_ocr.ps1` (see that file for the
//! exact line/word box shape this reproduces).

use tooltip::{Frame, OcrLine};
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::{OcrEngine, OcrLine as RawOcrLine};
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use crate::Error;

/// Windows' on-device OCR engine (`Windows.Media.Ocr.OcrEngine`); create once, reuse for every
/// capture (recreating it per call would repeat the ~0.5-0.8 s warm-up `win_ocr.py` avoids by
/// keeping one worker alive).
///
/// `Ocr` is `Send + Sync`: windows-rs marks the generated `OcrEngine` handle `Send + Sync`, and
/// the cached `language` is a plain `String`. That covers moving an `Ocr` to another thread, but
/// not WinRT's *per-thread* apartment requirement — a plain worker thread has no apartment until
/// something initializes one, unlike Tauri's WebView2 thread. Both `new` and `read` call
/// `RoInitialize` defensively so `Ocr` works from any thread, including one built solely to own it.
pub struct Ocr {
    engine: OcrEngine,
    language: String,
}

impl Ocr {
    /// Creates an OCR engine from the user's installed OCR languages.
    ///
    /// Returns `Error::NoOcrLanguage` if Windows has none installed (Settings > Time & Language >
    /// Language & region > a language's optional "Optical character recognition" feature) — the
    /// same condition `win_ocr.ps1` reports as `{"ok": false, "error": "Windows has no OCR
    /// language installed"}`.
    pub fn new() -> Result<Self, Error> {
        ensure_winrt_initialized()?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(no_language_or)?;
        let language = engine.RecognizerLanguage()?.LanguageTag()?.to_string();
        Ok(Self { engine, language })
    }

    /// The BCP-47 tag of the language being recognised, e.g. `"en-US"`.
    pub fn language(&self) -> String {
        self.language.clone()
    }

    /// Recognises text in `frame`, returning one [`OcrLine`] per OCR line, top to bottom.
    ///
    /// Each line's box is the union of its words' bounding rects, rounded to `i32` the same way
    /// `win_ocr.ps1`'s `[int]` cast does (round half to even), so results match it exactly.
    pub fn read(&self, frame: &Frame) -> Result<Vec<OcrLine>, Error> {
        if frame.width() == 0 || frame.height() == 0 {
            return Ok(Vec::new());
        }
        let max_dimension = OcrEngine::MaxImageDimension()? as usize;
        if frame.width() > max_dimension || frame.height() > max_dimension {
            let max = max_dimension as u32;
            return Err(Error::FrameTooLarge { width: frame.width(), height: frame.height(), max });
        }
        ensure_winrt_initialized()?;

        let bitmap = software_bitmap_from(frame)?;
        // `join` blocks this thread until `RecognizeAsync` completes (a kernel event wait, not a
        // message pump), matching win_ocr.ps1's synchronous `Await`/`$task.Wait()`.
        let result = self.engine.RecognizeAsync(&bitmap)?.join()?;

        let raw_lines = result.Lines()?;
        let mut lines = Vec::with_capacity(raw_lines.Size()? as usize);
        for line in raw_lines {
            let text = line.Text()?.to_string();
            let (x, y, w, h) = line_box(&line)?;
            lines.push(OcrLine::new(text, x, y, w, h));
        }
        Ok(lines)
    }
}

/// Maps `TryCreateFromUserProfileLanguages`'s "no language installed" outcome to a clear error.
///
/// That method reports "no language installed" by succeeding with a null engine, not by failing;
/// windows-rs surfaces a null success as an `Err` whose HRESULT is still S_OK (`code().is_ok()`),
/// which is how we tell it apart from a genuinely failing, non-zero HRESULT.
fn no_language_or(e: windows::core::Error) -> Error {
    if e.code().is_ok() { Error::NoOcrLanguage } else { Error::Windows(e) }
}

/// Copies `frame`'s pixels into a `SoftwareBitmap`, entirely in memory: no temp file and no PNG
/// encoding, unlike `win_ocr.ps1`, which OCRs a BMP decoded from disk.
fn software_bitmap_from(frame: &Frame) -> windows::core::Result<SoftwareBitmap> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(&frame.to_bgra())?;
    let buffer = writer.DetachBuffer()?;
    // `Ignore`: `Frame` is opaque RGB (alpha is always 255 in `to_bgra`), and OCR only reads colour.
    SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        frame.width() as i32,
        frame.height() as i32,
        BitmapAlphaMode::Ignore,
    )
}

/// The union of `line`'s words' bounding rects, as `(x, y, w, h)` rounded like `win_ocr.ps1`.
fn line_box(line: &RawOcrLine) -> windows::core::Result<(i32, i32, i32, i32)> {
    let words = line.Words()?;
    if words.Size()? == 0 {
        // Windows.Media.Ocr never actually returns a line with no words, but if it ever did, the
        // running min/max fold below would produce a garbage (i32::MAX, i32::MAX, i32::MIN,
        // i32::MIN) box instead of failing loudly, which is worse; an empty box is at least inert.
        return Ok((0, 0, 0, 0));
    }
    let (mut left, mut top) = (f64::MAX, f64::MAX);
    let (mut right, mut bottom): (f64, f64) = (0.0, 0.0);
    for word in words {
        let bounds = word.BoundingRect()?;
        let (x, y) = (bounds.X as f64, bounds.Y as f64);
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + bounds.Width as f64);
        bottom = bottom.max(y + bounds.Height as f64);
    }
    // Each value is rounded independently from the raw floats (not derived from an already-rounded
    // x/y), matching `[int]$left` / `[int]($right - $left)` in win_ocr.ps1 exactly.
    let (x, y) = (round_half_even(left), round_half_even(top));
    let (w, h) = (round_half_even(right - left), round_half_even(bottom - top));
    Ok((x, y, w, h))
}

/// Rounds like PowerShell's `[int]` cast on a `[double]`: round half to even ("banker's
/// rounding"), not Rust's default `f64::round`, which rounds half away from zero.
fn round_half_even(v: f64) -> i32 {
    v.round_ties_even() as i32
}

/// Makes sure the calling thread has a WinRT apartment, which every WinRT call needs and a plain
/// worker thread does not have until something initializes one.
fn ensure_winrt_initialized() -> Result<(), Error> {
    // SAFETY: `RoInitialize` takes no pointers; it only sets up this thread's COM/WinRT apartment
    // state, so it is safe to call unconditionally, including more than once per thread.
    match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        Ok(()) => Ok(()),
        // Some other component (e.g. the host's UI toolkit) already initialized this thread,
        // possibly with a different concurrency model; either way, a usable apartment exists.
        Err(e) if e.code() == RPC_E_CHANGED_MODE => Ok(()),
        Err(e) => Err(e.into()),
    }
}
