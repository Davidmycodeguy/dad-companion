//! A saved per-resolution calibration override, and applying it to a [`super::ScreenLayout`].
//!
//! Ports `macros.py`'s `_apply_calibration_override`. The JSON shape (field names and nesting)
//! matches the Python reference exactly, since this struct is meant to be stored verbatim under
//! the app's `calibrationOverride` settings key (the generic JSON settings store lives in the
//! `appdata` crate, out of scope here — this crate only defines the shape).

use serde::{Deserialize, Serialize};

use super::stash_tabs::STASH_TAB_COUNT;
use super::ScreenLayout;
use crate::Point;

/// The resolution a saved calibration was measured at; the override only applies when the current
/// resolution matches exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CalibratedResolution {
    pub width: i32,
    pub height: i32,
}

/// A pixel offset. Kept as `f64` (not [`Point`]) because it is added to a layout position before
/// any rounding happens — see the module docs on precision in `super::resolution`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
}

/// An unrounded saved point, e.g. one entry of `stashTabPositions`. Rounded Python-style (see
/// [`crate::round::python_round`]) only when read back via [`ScreenLayout::stash_tab_positions`],
/// matching where the Python reference rounds it.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RawPoint {
    pub x: f64,
    pub y: f64,
}

/// A saved calibration override for one resolution. Every field is optional/defaulted so a
/// partially-filled JSON object (as the settings UI would save while the user is still adjusting
/// one field) still deserializes, matching the Python reference's tolerant `dict.get(key, {})`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationOverride {
    #[serde(default)]
    pub resolution: Option<CalibratedResolution>,
    #[serde(default)]
    pub stash_delta: Delta,
    #[serde(default)]
    pub inv_delta: Delta,
    #[serde(default)]
    pub jump: Option<f64>,
    #[serde(default)]
    pub stash_tab_origin_delta: Delta,
    #[serde(default)]
    pub stash_tab_spacing: Option<f64>,
    #[serde(default)]
    pub stash_tab_positions: Option<Vec<RawPoint>>,
}

/// Applies `calibration` to `layout`, but only if it was saved for exactly `resolution` — a
/// calibration for one resolution must never leak into another. Ports
/// `_apply_calibration_override`; the "skip if every delta is zero and jump is absent" fast path
/// in the Python reference is not ported since adding a zero delta is a no-op anyway.
pub fn apply_calibration(layout: ScreenLayout, calibration: &CalibrationOverride, resolution: (u32, u32)) -> ScreenLayout {
    let matches = calibration
        .resolution
        .is_some_and(|r| r.width == resolution.0 as i32 && r.height == resolution.1 as i32);
    if !matches {
        return layout;
    }

    let stash = (layout.stash.0 + calibration.stash_delta.dx, layout.stash.1 + calibration.stash_delta.dy);
    let inv = (layout.inv.0 + calibration.inv_delta.dx, layout.inv.1 + calibration.inv_delta.dy);
    // Stash tab selectors sit next to the stash, so they shift by the stash delta *and* their own
    // independent delta from the settings page.
    let stash_tab_origin = (
        layout.stash_tab_origin.0 + calibration.stash_delta.dx + calibration.stash_tab_origin_delta.dx,
        layout.stash_tab_origin.1 + calibration.stash_delta.dy + calibration.stash_tab_origin_delta.dy,
    );

    ScreenLayout {
        stash,
        inv,
        jump: calibration.jump.unwrap_or(layout.jump),
        stash_tab_origin,
        stash_tab_spacing: calibration.stash_tab_spacing.unwrap_or(layout.stash_tab_spacing),
        stash_tab_positions_override: calibration.stash_tab_positions.clone(),
    }
}

/// Rounds a saved override list to physical points, keeping only the first [`STASH_TAB_COUNT`].
/// `None` if there are fewer than `STASH_TAB_COUNT` saved (the Python reference falls back to the
/// computed positions in that case).
pub(super) fn rounded_override(saved: &Option<Vec<RawPoint>>) -> Option<Vec<Point>> {
    let saved = saved.as_ref()?;
    if saved.len() < STASH_TAB_COUNT {
        return None;
    }
    Some(saved[..STASH_TAB_COUNT].iter().map(|p| Point::rounded(p.x, p.y)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_layout() -> ScreenLayout {
        ScreenLayout {
            stash: (1378.0, 199.0),
            inv: (690.0, 626.0),
            jump: 40.5,
            stash_tab_origin: (1328.0, 211.0),
            stash_tab_spacing: 45.0,
            stash_tab_positions_override: None,
        }
    }

    #[test]
    fn mismatched_resolution_is_ignored() {
        let cal = CalibrationOverride {
            resolution: Some(CalibratedResolution { width: 2560, height: 1440 }),
            stash_delta: Delta { dx: 100.0, dy: 100.0 },
            ..Default::default()
        };
        let result = apply_calibration(base_layout(), &cal, (1920, 1080));
        assert_eq!(result, base_layout());
    }

    #[test]
    fn deltas_and_jump_apply_when_resolution_matches() {
        let cal = CalibrationOverride {
            resolution: Some(CalibratedResolution { width: 1920, height: 1080 }),
            stash_delta: Delta { dx: 2.0, dy: -3.0 },
            inv_delta: Delta { dx: 1.0, dy: 1.0 },
            jump: Some(41.0),
            stash_tab_origin_delta: Delta { dx: 0.0, dy: 5.0 },
            stash_tab_spacing: Some(46.0),
            stash_tab_positions: None,
        };
        let result = apply_calibration(base_layout(), &cal, (1920, 1080));
        assert_eq!(result.stash, (1380.0, 196.0));
        assert_eq!(result.inv, (691.0, 627.0));
        assert_eq!(result.jump, 41.0);
        // stash delta (2, -3) plus its own delta (0, 5).
        assert_eq!(result.stash_tab_origin, (1330.0, 213.0));
        assert_eq!(result.stash_tab_spacing, 46.0);
    }

    #[test]
    fn no_calibration_saved_leaves_layout_untouched() {
        let result = apply_calibration(base_layout(), &CalibrationOverride::default(), (1920, 1080));
        assert_eq!(result, base_layout());
    }
}
