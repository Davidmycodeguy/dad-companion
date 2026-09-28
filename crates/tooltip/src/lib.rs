//! The game's item tooltip: finding it in screen pixels (`finder`) and turning its OCR'd text into
//! an item with its rolls (`parser`). Pure logic over plain data; capturing the screen and running
//! OCR live in the `screen` crate.

pub mod finder;
pub mod frame;
pub mod parser;
pub mod reader;

pub use frame::{Frame, OcrLine, Region};
