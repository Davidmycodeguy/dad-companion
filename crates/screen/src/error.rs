//! The crate's single error type.

use thiserror::Error;

/// Everything that can go wrong in screen capture, OCR and the window/cursor helpers.
///
/// Win32 and WinRT calls report failure as an HRESULT; wrapping `windows::core::Error` instead of
/// converting it to a string up front preserves that HRESULT (and, for WinRT, any extended error
/// text) for callers that want to inspect it, while `Display` already renders a human-readable
/// message plus the HRESULT, e.g. "Access is denied. (0x80070005)".
#[derive(Debug, Error)]
pub enum Error {
    /// A Win32 or WinRT call failed.
    #[error(transparent)]
    Windows(#[from] windows::core::Error),

    /// `OcrEngine::TryCreateFromUserProfileLanguages` succeeded but returned no engine, which is
    /// how Windows reports "no OCR language is installed" (it is not an HRESULT failure, so it
    /// cannot be represented as an `Error::Windows` with a useful message: see `Ocr::new`).
    #[error("Windows has no OCR language installed")]
    NoOcrLanguage,

    /// The frame is larger than `OcrEngine::MaxImageDimension` in width or height, so
    /// `RecognizeAsync` would reject it.
    #[error("frame is {width}x{height}, which exceeds the OCR engine's max dimension of {max}")]
    FrameTooLarge {
        /// The frame's width in pixels.
        width: usize,
        /// The frame's height in pixels.
        height: usize,
        /// `OcrEngine::MaxImageDimension()`, in pixels, for either axis.
        max: u32,
    },
}
