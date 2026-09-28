//! Python-compatible rounding.
//!
//! Every screen coordinate in the Python reference passes through a builtin `round()` at some
//! point (`screen_scaling.scale_point`, `MarketplaceLayout._offset`, `move_mouse_smooth`, ...).
//! Python 3's `round()` breaks exact `.5` ties to the nearest *even* integer ("banker's rounding"),
//! not away from zero like Rust's `f64::round()`. The two disagree exactly on ties, which the
//! Python test suite depends on: `test_build_layout_1080p_base_points` asserts
//! `tab_icon(1) == (1315, 242)` for `196 + 46.5 == 242.5`, which rounds *down* to the even 242.
//! Using `f64::round()` here would silently mis-click by a pixel on every half-pixel boundary.
pub fn python_round(x: f64) -> i64 {
    let floor = x.floor();
    let fract = x - floor;
    let floor_i = floor as i64;
    if fract < 0.5 {
        floor_i
    } else if fract > 0.5 {
        floor_i + 1
    } else if floor_i % 2 == 0 {
        floor_i
    } else {
        floor_i + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_round_to_even() {
        assert_eq!(python_round(242.5), 242);
        assert_eq!(python_round(243.5), 244);
        assert_eq!(python_round(0.5), 0);
        assert_eq!(python_round(1.5), 2);
        assert_eq!(python_round(-0.5), 0);
        assert_eq!(python_round(-1.5), -2);
    }

    #[test]
    fn non_ties_round_to_nearest() {
        assert_eq!(python_round(242.4), 242);
        assert_eq!(python_round(242.6), 243);
        assert_eq!(python_round(-242.6), -243);
    }

    #[test]
    fn whole_numbers_are_unchanged() {
        assert_eq!(python_round(54.0), 54);
        assert_eq!(python_round(0.0), 0);
    }
}
