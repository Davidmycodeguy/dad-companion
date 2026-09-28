//! Per-resolution stash/inventory/stash-tab positions.
//!
//! Ports `macros.py`'s `BASE_LAYOUT`, `MANUAL_OVERRIDES`, `_scaled_layout` and
//! `get_positions_for_resolution`.
//!
//! # Why positions stay `f64` until the last possible moment
//!
//! `stash`/`inv`/`stash_tab_origin` are kept as `(f64, f64)`, not [`Point`], even though the base
//! layout and manual overrides are exact integers. The Python reference lets a calibration delta
//! leave these fractional (`Point(positions['stash'].x + dx_s, ...)` with a float `dx_s`), and
//! that fractional value then flows straight into `move_from_to_reliable`'s own arithmetic
//! (`base_screen_pos.x + jump * pos.x + ...`) before anything is rounded. Rounding early here
//! would round twice and could be off by a pixel from the Python reference on the rare unlucky
//! fraction. Only `stash_tab_positions()` rounds — the one place the Python reference does too.

use super::calibration::rounded_override;
use super::stash_tabs::STASH_TAB_COUNT;
use crate::Point;

/// The resolution `BASE_LAYOUT`'s pixel values were measured at.
pub const BASE_RESOLUTION: (u32, u32) = (1920, 1080);

/// One resolution's screen positions: where the stash/inventory windows are, the per-cell pixel
/// size (`jump`), and where the stash tab selectors start. Ports the dict `macros.py` builds in
/// `get_positions_for_resolution`/`get_screen_positions`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenLayout {
    /// Top-left of the stash grid, in physical screen pixels.
    pub stash: (f64, f64),
    /// Top-left of the inventory grid, in physical screen pixels.
    pub inv: (f64, f64),
    /// Pixel size of one stash/inventory cell (both axes: cells are square).
    pub jump: f64,
    /// Centre of the first (topmost) stash tab selector.
    pub stash_tab_origin: (f64, f64),
    /// Vertical pixel gap between consecutive stash tab selector centres.
    pub stash_tab_spacing: f64,
    /// Individually-saved tab positions from a calibration session, if any, preferred over
    /// `stash_tab_origin` + `stash_tab_spacing` when present. Set only by
    /// [`super::apply_calibration`].
    pub stash_tab_positions_override: Option<Vec<super::calibration::RawPoint>>,
}

impl ScreenLayout {
    /// The [`STASH_TAB_COUNT`] stash tab selector centres, individually-saved ones preferred over
    /// `stash_tab_origin + index * stash_tab_spacing`. Ports `get_stash_tab_positions`.
    pub fn stash_tab_positions(&self) -> Vec<Point> {
        if let Some(saved) = rounded_override(&self.stash_tab_positions_override) {
            return saved;
        }
        (0..STASH_TAB_COUNT)
            .map(|i| Point::rounded(self.stash_tab_origin.0, self.stash_tab_origin.1 + i as f64 * self.stash_tab_spacing))
            .collect()
    }
}

/// The 1920x1080 calibration every other resolution is scaled from. Ports `BASE_LAYOUT`.
const BASE_LAYOUT: ScreenLayout = ScreenLayout {
    stash: (1378.0, 199.0),
    inv: (690.0, 626.0),
    jump: 40.5,
    stash_tab_origin: (1328.0, 211.0),
    stash_tab_spacing: 45.0,
    stash_tab_positions_override: None,
};

/// Hand-tuned positions for resolutions where plain scaling drifts (fullscreen 720p was measured
/// directly rather than scaled). Ports `MANUAL_OVERRIDES`.
const MANUAL_OVERRIDES: &[((u32, u32), ScreenLayout)] = &[(
    (1280, 720),
    ScreenLayout {
        stash: (918.0, 132.0),
        inv: (457.0, 416.0),
        jump: 27.0,
        stash_tab_origin: (881.0, 139.0),
        stash_tab_spacing: 31.0,
        stash_tab_positions_override: None,
    },
)];

/// `BASE_LAYOUT` scaled to `resolution`. Ports `_scaled_layout`.
fn scaled_layout(resolution: (u32, u32)) -> ScreenLayout {
    let scale = super::scale_for(resolution);
    let point = |base: (f64, f64)| -> (f64, f64) {
        let (x, y) = super::scale_point(base.0, base.1, scale);
        (x as f64, y as f64)
    };
    ScreenLayout {
        stash: point(BASE_LAYOUT.stash),
        inv: point(BASE_LAYOUT.inv),
        jump: super::scale_length(BASE_LAYOUT.jump, scale),
        stash_tab_origin: point(BASE_LAYOUT.stash_tab_origin),
        stash_tab_spacing: super::scale_length(BASE_LAYOUT.stash_tab_spacing, scale),
        stash_tab_positions_override: None,
    }
}

/// Stash/inventory/stash-tab positions for `resolution`: a manual override if one exists for it,
/// otherwise `BASE_LAYOUT` scaled to it. Ports `get_positions_for_resolution`. Unlike the Python
/// reference this is not cached in a global `RESOLUTION_POSITIONS` dict — the computation is cheap
/// arithmetic, so there is nothing worth caching (and no global mutable state to reason about).
pub fn positions_for_resolution(resolution: (u32, u32)) -> ScreenLayout {
    if let Some((_, layout)) = MANUAL_OVERRIDES.iter().find(|(res, _)| *res == resolution) {
        return layout.clone();
    }
    scaled_layout(resolution)
}

/// Shifts `stash`/`inv`/`stash_tab_origin` by the game window's client-area top-left, for windowed
/// mode where the base layout is measured relative to the client area, not the screen. Ports the
/// `window_left`/`window_top` addition inlined in `get_screen_positions`.
pub fn apply_window_offset(layout: ScreenLayout, offset: (i32, i32)) -> ScreenLayout {
    let (dx, dy) = (offset.0 as f64, offset.1 as f64);
    ScreenLayout {
        stash: (layout.stash.0 + dx, layout.stash.1 + dy),
        inv: (layout.inv.0 + dx, layout.inv.1 + dy),
        stash_tab_origin: (layout.stash_tab_origin.0 + dx, layout.stash_tab_origin.1 + dy),
        ..layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_resolution_is_unscaled() {
        let layout = positions_for_resolution(BASE_RESOLUTION);
        assert_eq!(layout.stash, (1378.0, 199.0));
        assert_eq!(layout.inv, (690.0, 626.0));
        assert_eq!(layout.jump, 40.5);
    }

    #[test]
    fn manual_override_is_used_verbatim_for_720p() {
        let layout = positions_for_resolution((1280, 720));
        assert_eq!(layout.stash, (918.0, 132.0));
        assert_eq!(layout.jump, 27.0);
    }

    #[test]
    fn four_k_scales_everything_by_two() {
        let layout = positions_for_resolution((3840, 2160));
        assert_eq!(layout.stash, (2756.0, 398.0));
        assert_eq!(layout.jump, 81.0);
    }

    #[test]
    fn window_offset_shifts_click_points_but_not_jump_or_spacing() {
        let layout = apply_window_offset(positions_for_resolution(BASE_RESOLUTION), (100, 50));
        assert_eq!(layout.stash, (1478.0, 249.0));
        assert_eq!(layout.stash_tab_origin, (1428.0, 261.0));
        assert_eq!(layout.jump, 40.5);
    }

    #[test]
    fn stash_tab_positions_fall_back_to_origin_plus_spacing() {
        let layout = positions_for_resolution(BASE_RESOLUTION);
        let tabs = layout.stash_tab_positions();
        assert_eq!(tabs.len(), STASH_TAB_COUNT);
        assert_eq!(tabs[0], Point::new(1328, 211));
        assert_eq!(tabs[1], Point::new(1328, 256));
        assert_eq!(tabs[7], Point::new(1328, 211 + 45 * 7));
    }
}
