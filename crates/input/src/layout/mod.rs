//! Per-resolution screen layout: scaling, base positions, calibration and the Marketplace screen.
//!
//! Ports the screen-position half of `macros.py` (`BASE_LAYOUT`, `MANUAL_OVERRIDES`,
//! `get_positions_for_resolution`, `_apply_calibration_override`, stash tab positions),
//! `screen_scaling.py` in full, and `marketplace_layout.py` in full (see [`marketplace`]).

mod calibration;
pub mod marketplace;
mod resolution;
mod stash_tabs;

pub use calibration::{apply_calibration, CalibratedResolution, CalibrationOverride, Delta, RawPoint};
pub use resolution::{apply_window_offset, positions_for_resolution, ScreenLayout, BASE_RESOLUTION};
pub use stash_tabs::{stash_type_name, tab_index_for_stash_type, DEFAULT_STASH_TAB_MAPPING, STASH_TAB_COUNT};

/// The 16:9 aspect ratio (`1920x1080`) the whole calibration is measured against.
pub const STANDARD_ASPECT: f64 = 16.0 / 9.0;

/// Per-axis scale plus a horizontal offset, computed once per resolution and reused for every
/// point/length in that resolution's layout. Ports `screen_scaling.Scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub sx: f64,
    pub sy: f64,
    pub offset_x: f64,
}

/// True when `resolution` is wider than 16:9, with the same `0.01` slack `macros.py` and
/// `screen_scaling.py` both use so an exact 16:9 resolution never misclassifies from float error.
/// Ports `macros.py`'s `_is_ultrawide` (`screen_scaling.scale_for` inlines the same check).
pub fn is_ultrawide(resolution: (u32, u32)) -> bool {
    let (w, h) = (resolution.0 as f64, resolution.1.max(1) as f64);
    (w / h) > (STANDARD_ASPECT + 0.01)
}

/// The scale (and, for ultrawide, horizontal letterbox offset) that maps the 1920x1080 base
/// layout onto `resolution`. Ports `screen_scaling.scale_for`.
pub fn scale_for(resolution: (u32, u32)) -> Scale {
    let (w, h) = (resolution.0 as f64, resolution.1 as f64);
    if is_ultrawide(resolution) {
        let s = h / BASE_RESOLUTION.1 as f64;
        Scale { sx: s, sy: s, offset_x: (w - h * STANDARD_ASPECT) / 2.0 }
    } else {
        Scale { sx: w / BASE_RESOLUTION.0 as f64, sy: h / BASE_RESOLUTION.1 as f64, offset_x: 0.0 }
    }
}

/// Scales a 1920x1080-base point to `scale`'s resolution, rounding Python-style. Ports
/// `screen_scaling.scale_point`.
pub fn scale_point(x: f64, y: f64, scale: Scale) -> (i32, i32) {
    (
        crate::round::python_round(x * scale.sx + scale.offset_x) as i32,
        crate::round::python_round(y * scale.sy) as i32,
    )
}

/// Scales a 1920x1080-base length, floored at 1 pixel so nothing vanishes at very small scales.
/// Not rounded to an integer — lengths (e.g. `jump`) stay fractional. Ports
/// `screen_scaling.scale_length`.
pub fn scale_length(value: f64, scale: Scale) -> f64 {
    (value * scale.sy).max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_for_1080p_and_4k_is_exact() {
        assert_eq!(scale_for((1920, 1080)), Scale { sx: 1.0, sy: 1.0, offset_x: 0.0 });
        assert_eq!(scale_for((3840, 2160)), Scale { sx: 2.0, sy: 2.0, offset_x: 0.0 });
    }

    #[test]
    fn scale_for_ultrawide_letterboxes_horizontally() {
        let scale = scale_for((3440, 1440));
        assert!((scale.sx - 1440.0 / 1080.0).abs() < 1e-9);
        assert!((scale.offset_x - (3440.0 - 1440.0 * 16.0 / 9.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn scale_point_matches_python_reference() {
        // BASE_LAYOUT['stash'] = (1378, 199); 4K -> (2756, 398).
        assert_eq!(scale_point(1378.0, 199.0, scale_for((3840, 2160))), (2756, 398));
    }

    #[test]
    fn scale_length_floors_at_one_pixel() {
        assert!((scale_length(40.5, scale_for((2560, 1440))) - 54.0).abs() < 1e-9);
        assert_eq!(scale_length(0.1, scale_for((1280, 720))), 1.0);
    }

    #[test]
    fn is_ultrawide_classification() {
        assert!(!is_ultrawide((1920, 1080)));
        assert!(!is_ultrawide((3840, 2160)));
        assert!(is_ultrawide((3440, 1440)));
        assert!(is_ultrawide((2560, 1080)));
    }
}
