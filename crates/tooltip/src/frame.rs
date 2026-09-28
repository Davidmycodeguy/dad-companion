//! Plain data shared by the finder, the parser and the `screen` crate.

/// A screen region in physical pixels: (left, top, right, bottom), right and bottom exclusive.
pub type Region = (i32, i32, i32, i32);

/// An RGB image: rows top to bottom, 3 bytes (R, G, B) per pixel, no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

impl Frame {
    /// A frame from RGB bytes; `data.len()` must be `width * height * 3`.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width * height * 3, "RGB data does not match {width}x{height}");
        Self { width, height, data }
    }

    /// A frame of one colour.
    pub fn filled(width: usize, height: usize, rgb: [u8; 3]) -> Self {
        Self::new(width, height, rgb.repeat(width * height))
    }

    /// A frame from 32-bit BGRA rows (as GDI and Windows imaging hand them over), dropping alpha.
    pub fn from_bgra(width: usize, height: usize, bgra: &[u8]) -> Self {
        assert_eq!(bgra.len(), width * height * 4, "BGRA data does not match {width}x{height}");
        let mut data = Vec::with_capacity(width * height * 3);
        for &[b, g, r, _] in bgra.as_chunks::<4>().0 {
            data.extend_from_slice(&[r, g, b]);
        }
        Self { width, height, data }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The RGB bytes, rows top to bottom.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// One row's RGB bytes.
    pub fn row(&self, y: usize) -> &[u8] {
        &self.data[y * self.width * 3..(y + 1) * self.width * 3]
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        let i = (y * self.width + x) * 3;
        self.data[i..i + 3].copy_from_slice(&rgb);
    }

    /// Fills the rectangle [left, right) x [top, bottom), clipped to the frame.
    pub fn fill_rect(&mut self, left: usize, top: usize, right: usize, bottom: usize, rgb: [u8; 3]) {
        for y in top..bottom.min(self.height) {
            for x in left..right.min(self.width) {
                self.set_pixel(x, y, rgb);
            }
        }
    }

    /// The pixels of [left, right) x [top, bottom), clipped to the frame.
    pub fn crop(&self, left: usize, top: usize, right: usize, bottom: usize) -> Frame {
        let (right, bottom) = (right.min(self.width), bottom.min(self.height));
        let (width, height) = (right.saturating_sub(left), bottom.saturating_sub(top));
        let mut data = Vec::with_capacity(width * height * 3);
        for y in top..top + height {
            data.extend_from_slice(&self.row(y)[left * 3..right * 3]);
        }
        Frame { width, height, data }
    }

    /// 32-bit BGRA bytes (alpha 255), for Windows imaging (OCR).
    pub fn to_bgra(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for &[r, g, b] in self.data.as_chunks::<3>().0 {
            out.extend_from_slice(&[b, g, r, 255]);
        }
        out
    }
}

/// One line of text found by OCR, with its box in the image's pixels (from its words' boxes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrLine {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl OcrLine {
    pub fn new(text: impl Into<String>, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { text: text.into(), x, y, w, h }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_round_trip_and_crop() {
        let frame = Frame::from_bgra(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(frame.pixel(0, 0), [3, 2, 1]);
        assert_eq!(frame.to_bgra(), [1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(frame.crop(1, 0, 5, 5).pixel(0, 0), [6, 5, 4]);
    }

    #[test]
    fn fill_rect_is_clipped() {
        let mut frame = Frame::filled(4, 4, [0, 0, 0]);
        frame.fill_rect(2, 2, 10, 10, [9, 9, 9]);
        assert_eq!((frame.pixel(3, 3), frame.pixel(1, 1)), ([9, 9, 9], [0, 0, 0]));
    }
}
