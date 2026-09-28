//! The hover test: moves the mouse over each Marketplace spot without clicking, so the player can
//! check the calibration by eye. Port of the `hover()` action of DnDTools' `_lister_hover_factory`
//! (the live version, which first brings the game forward, is [`super::live::live_hover_test`]).

use std::time::Duration;

use super::{GameInput, InputError, ScreenPoint};
use crate::job::CancelToken;

/// How long the cursor rests on each spot.
pub const HOVER_DWELL: Duration = Duration::from_secs(1);

/// Moves over each of `targets` in turn, resting [`HOVER_DWELL`] on each through `wait` (which
/// should return early once `cancel` is cancelled), until done or cancelled. Never clicks.
pub fn hover_over(
    driver: &dyn GameInput,
    targets: &[(&str, ScreenPoint)],
    cancel: &CancelToken,
    wait: &dyn Fn(Duration),
) -> Result<(), InputError> {
    for &(_, point) in targets {
        if cancel.is_cancelled() {
            return Ok(());
        }
        driver.move_to(point)?;
        wait(HOVER_DWELL);
    }
    Ok(())
}
