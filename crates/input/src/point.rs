//! Ports `point.py`'s `Point`: an integer screen-pixel coordinate.

use serde::{Deserialize, Serialize};

/// A physical screen-pixel coordinate. Mirrors `point.py`'s `Point`, which held plain (possibly
/// Python-`int`) `x`/`y` fields with structural equality; `Copy` here since it is two `i32`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    /// A new point at `(x, y)`.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// `(x, y)` rounded Python-style (see [`crate::round::python_round`]) to the nearest point.
    /// Used wherever the Python reference does `Point(int(round(x)), int(round(y)))`.
    pub fn rounded(x: f64, y: f64) -> Self {
        Self {
            x: crate::round::python_round(x) as i32,
            y: crate::round::python_round(y) as i32,
        }
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Point({}, {})", self.x, self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_is_structural() {
        assert_eq!(Point::new(1, 2), Point::new(1, 2));
        assert_ne!(Point::new(1, 2), Point::new(2, 1));
    }

    #[test]
    fn rounded_uses_python_style_rounding() {
        assert_eq!(Point::rounded(1268.5, 242.5), Point::new(1268, 242));
    }

    #[test]
    fn display_matches_python_repr() {
        assert_eq!(Point::new(3, 4).to_string(), "Point(3, 4)");
    }
}
