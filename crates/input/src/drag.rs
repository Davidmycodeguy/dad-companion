//! `move_from_to_reliable`: the stash-cell drag, with its double-click protection and "reliability
//! click" safety net. Ports the same-named function in `macros.py`.

use std::time::{Duration, Instant};

use crate::mouse::{move_mouse_smooth, SmoothMoveOptions};
use crate::sink::InputSink;
use crate::{Cancel, Error, Point};

/// Floor pause used in "instant" (zero sort-delay) mode so SendInput calls still land reliably.
/// Ports `INSTANT_MODE_MIN_PAUSE` (`0.004`).
const INSTANT_MODE_MIN_PAUSE: Duration = Duration::from_micros(4_000);
/// How long after a pickup/drop the same cell is protected against a second, likely-accidental
/// click. Ports `DOUBLE_CLICK_PROTECT_WINDOW` (`0.18`).
const DOUBLE_CLICK_PROTECT_WINDOW: Duration = Duration::from_millis(180);

const JITTER_STEP: Duration = Duration::from_millis(70);
const JITTER_RELIABILITY: Duration = Duration::from_millis(20);
const JITTER_RELIABILITY_HALF: Duration = Duration::from_millis(15);
const JITTER_SKIP: Duration = Duration::from_millis(5);
const TRAVEL_STEP_DELAY: Duration = Duration::from_micros(1_000);
const TRAVEL_STEP_DELAY_MAX: Duration = Duration::from_micros(3_000);

/// One stash cell, identified well enough to debounce accidental double-clicks on it.
///
/// Ports the `(id(stash), pos.x, pos.y)` signature tuple the Python reference builds from a
/// `Stash` object's identity. This crate has no `Stash` type of its own, so `stash_id` is instead
/// any value the caller chooses that uniquely and consistently identifies one stash/inventory box
/// for the lifetime of a [`DragState`] — e.g. a small per-box index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DragSlot {
    pub stash_id: u64,
    pub cell: Point,
}

/// One end of a drag: which cell, in which box, and how big the item there is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragEndpoint {
    pub slot: DragSlot,
    /// The box's top-left in physical screen pixels (`base_screen_pos` in the Python reference).
    pub origin: (f64, f64),
    pub width: i32,
    pub height: i32,
}

/// The centre, in physical screen pixels, of `cell` (`width` x `height` cells) within a box whose
/// top-left is `origin`, given the per-cell pixel size `jump`. Ports the arithmetic
/// `move_from_to_reliable` inlines for both its start and end positions.
pub fn cell_centre(origin: (f64, f64), jump: f64, cell: Point, width: i32, height: i32) -> (f64, f64) {
    (origin.0 + jump * cell.x as f64 + jump * width as f64 / 2.0, origin.1 + jump * cell.y as f64 + jump * height as f64 / 2.0)
}

/// Double-click protection state carried between [`move_from_to_reliable`] calls. Ports the
/// module-level `_last_pickup_signature`/`_last_drop_signature`/`*_time` globals as an explicit,
/// per-caller value instead — there is no reason for every macro on every thread to share one
/// global in Rust, and doing so would make concurrent sorts interfere with each other.
#[derive(Debug, Clone, Default)]
pub struct DragState {
    last_pickup: Option<(DragSlot, Instant)>,
    last_drop: Option<(DragSlot, Instant)>,
}

impl DragState {
    pub fn new() -> Self {
        Self::default()
    }
}

fn recently(entry: Option<(DragSlot, Instant)>, slot: DragSlot, now: Instant) -> bool {
    entry.is_some_and(|(seen, at)| seen == slot && now.saturating_duration_since(at) < DOUBLE_CLICK_PROTECT_WINDOW)
}

/// Timing shared by both legs of a drag, bundled so [`move_from_to_reliable`] stays at 7
/// parameters (clippy's `too_many_arguments` threshold).
#[derive(Debug, Clone, Copy)]
struct DragTiming {
    jump: f64,
    delay: Duration,
    no_delay_mode: bool,
}

/// Which mouse buttons this drag is currently holding down, so a cancellation partway through
/// still releases them (ports the Python reference's `try`/`finally`).
#[derive(Debug, Default)]
struct HeldButtons {
    primary: bool,
    reliability: bool,
}

/// Drags the item at `start` to `end`: press at `start`, move to `end`, release, then click once
/// more in place ("reliability click") to make sure the game registers the drop — unless the last
/// drop was the same cell within [`DOUBLE_CLICK_PROTECT_WINDOW`], in which case the reliability
/// click is skipped to avoid a double-click. Releases the mouse button if cancelled partway
/// through. Ports `move_from_to_reliable`.
pub fn move_from_to_reliable(
    sink: &mut dyn InputSink,
    cancel: &Cancel,
    jump: f64,
    delay: Duration,
    start: DragEndpoint,
    end: DragEndpoint,
    state: &mut DragState,
) -> Result<(), Error> {
    let timing = DragTiming { jump, delay, no_delay_mode: delay.is_zero() };
    let mut held = HeldButtons::default();
    let result = drag_body(sink, cancel, &timing, start, end, state, &mut held);
    // Safety net: release anything still held, exactly as the Python `finally` block does,
    // ignoring further errors from doing so (there's nothing left to recover into).
    if held.reliability {
        sink.mouse_up();
    }
    if held.primary {
        sink.mouse_up();
    }
    result
}

fn maybe_sleep(cancel: &Cancel, timing: &DragTiming, base_delay: Duration, jitter_max: Duration, enforce_floor: bool) -> Result<(), Error> {
    cancel.check()?;
    let total = if timing.no_delay_mode {
        if enforce_floor { INSTANT_MODE_MIN_PAUSE } else { INSTANT_MODE_MIN_PAUSE.mul_f64(0.5) }
    } else {
        let jitter_component = if jitter_max.is_zero() { Duration::ZERO } else { jitter_max.mul_f64(crate::jitter::unit_f64()) };
        base_delay + jitter_component
    };
    if total.is_zero() { cancel.check() } else { cancel.sleep(total) }
}

fn drag_body(
    sink: &mut dyn InputSink,
    cancel: &Cancel,
    timing: &DragTiming,
    start: DragEndpoint,
    end: DragEndpoint,
    state: &mut DragState,
    held: &mut HeldButtons,
) -> Result<(), Error> {
    let (sx, sy) = cell_centre(start.origin, timing.jump, start.slot.cell, start.width, start.height);
    let (ex, ey) = cell_centre(end.origin, timing.jump, end.slot.cell, end.width, end.height);
    let max_travel = (ex - sx).abs().max((ey - sy).abs());
    let small_move = max_travel <= (timing.jump * 0.6).max(12.0);
    let travel_opts = SmoothMoveOptions { min_delay: TRAVEL_STEP_DELAY, max_delay: TRAVEL_STEP_DELAY_MAX, no_delay: timing.no_delay_mode, steps: 0 };

    let (cx, cy) = sink.cursor_position();
    move_mouse_smooth(sink, cancel, (cx as f64, cy as f64), (sx, sy), SmoothMoveOptions { steps: if small_move { 24 } else { 20 }, ..travel_opts })?;
    maybe_sleep(cancel, timing, timing.delay, JITTER_STEP, true)?;

    if timing.no_delay_mode {
        let now = Instant::now();
        if recently(state.last_pickup, start.slot, now) || recently(state.last_drop, start.slot, now) {
            cancel.sleep(DOUBLE_CLICK_PROTECT_WINDOW)?;
        }
    }

    cancel.check()?;
    sink.mouse_down();
    held.primary = true;
    maybe_sleep(cancel, timing, timing.delay, JITTER_STEP, true)?;

    move_mouse_smooth(sink, cancel, (sx, sy), (ex, ey), SmoothMoveOptions { steps: if small_move { 28 } else { 25 }, ..travel_opts })?;
    maybe_sleep(cancel, timing, timing.delay, JITTER_STEP, true)?;

    cancel.check()?;
    sink.mouse_up();
    held.primary = false;
    maybe_sleep(cancel, timing, timing.delay, JITTER_STEP, true)?;

    let now = Instant::now();
    if recently(state.last_drop, end.slot, now) {
        maybe_sleep(cancel, timing, INSTANT_MODE_MIN_PAUSE, JITTER_SKIP, true)?;
    } else {
        move_mouse_smooth(sink, cancel, (ex, ey), (ex, ey), SmoothMoveOptions { steps: 5, min_delay: Duration::from_micros(500), max_delay: Duration::from_micros(1_500), no_delay: timing.no_delay_mode })?;
        let base_reliability_delay = if timing.delay > Duration::ZERO { timing.delay / 2 } else { Duration::ZERO };
        maybe_sleep(cancel, timing, base_reliability_delay, JITTER_RELIABILITY, true)?;
        cancel.check()?;
        sink.mouse_down();
        held.reliability = true;
        maybe_sleep(cancel, timing, base_reliability_delay / 2, JITTER_RELIABILITY_HALF, true)?;
        cancel.check()?;
        sink.mouse_up();
        held.reliability = false;
        maybe_sleep(cancel, timing, base_reliability_delay, JITTER_RELIABILITY, true)?;
    }

    state.last_pickup = Some((start.slot, now));
    state.last_drop = Some((end.slot, now));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_centre_matches_python_arithmetic() {
        // A 2x2 item at cell (1, 1) in a box whose top-left is (100, 200), jump 40.
        assert_eq!(cell_centre((100.0, 200.0), 40.0, Point::new(1, 1), 2, 2), (180.0, 280.0));
    }
}
