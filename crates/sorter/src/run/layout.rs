//! The screen layout a run uses. Port of `macros.get_screen_positions`.

use input::{apply_calibration, apply_window_offset, positions_for_resolution, CalibrationOverride, ScreenLayout, WindowRect};

/// Stash, inventory and tab positions for the game at `resolution`: the stock layout for that
/// resolution, shifted by the game window's client area when that area is exactly the resolution
/// (a windowed game, or a fullscreen game on a monitor that isn't at the desktop's origin), then the
/// player's calibration when it was saved for this resolution.
///
/// Python applied the window offset in windowed mode only; applying it whenever the client area
/// matches also places a fullscreen game on a second monitor correctly, and changes nothing for a
/// fullscreen game on the main monitor (its client area starts at 0, 0).
pub fn layout_for(resolution: (u32, u32), client: Option<WindowRect>, calibration: Option<&CalibrationOverride>) -> ScreenLayout {
    let mut layout = positions_for_resolution(resolution);
    if let Some(area) = client {
        if i64::from(area.width) == i64::from(resolution.0) && i64::from(area.height) == i64::from(resolution.1) {
            layout = apply_window_offset(layout, (area.left, area.top));
        }
    }
    match calibration {
        Some(calibration) => apply_calibration(layout, calibration, resolution),
        None => layout,
    }
}
