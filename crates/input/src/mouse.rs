//! Smooth mouse movement and cursor nudging: the mouse-macro pieces of `macros.py` layered on top
//! of the raw [`InputSink`] primitives.

use std::time::Duration;

use crate::round::python_round;
use crate::sink::InputSink;
use crate::{jitter, Cancel, Error};

/// `nudge_cursor`'s Python default (`dx=15, dy=0`).
pub const DEFAULT_NUDGE: (i32, i32) = (15, 0);

/// How a [`move_mouse_smooth`] call is paced. Bundled into one struct rather than passing
/// `move_mouse_smooth`'s four trailing Python parameters (`steps`, `min_delay`, `max_delay`,
/// `no_delay`) separately, to stay under clippy's argument-count lint and keep call sites
/// self-documenting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothMoveOptions {
    pub steps: u32,
    pub min_delay: Duration,
    pub max_delay: Duration,
    /// Skip the per-step sleep — movement is still interpolated over `steps`, just not paced.
    /// Used for "instant" sort-speed mode.
    pub no_delay: bool,
}

impl Default for SmoothMoveOptions {
    /// Matches `move_mouse_smooth`'s Python defaults: `steps=25, min_delay=0.008, max_delay=0.01`.
    fn default() -> Self {
        Self {
            steps: 25,
            min_delay: Duration::from_secs_f64(0.008),
            max_delay: Duration::from_secs_f64(0.01),
            no_delay: false,
        }
    }
}

/// Moves the mouse from `from` to `to` in a straight line over `opts.steps` steps, sleeping a
/// jittered delay between each unless `opts.no_delay`. Checked for cancellation before the first
/// step and again before every subsequent one. Ports `move_mouse_smooth`.
pub fn move_mouse_smooth(
    sink: &mut dyn InputSink,
    cancel: &Cancel,
    from: (f64, f64),
    to: (f64, f64),
    opts: SmoothMoveOptions,
) -> Result<(), Error> {
    cancel.check()?;
    let steps = opts.steps.max(1);
    let (x1, y1) = from;
    let (x2, y2) = to;
    for i in 1..=steps {
        cancel.check()?;
        let t = f64::from(i) / f64::from(steps);
        let x = x1 + (x2 - x1) * t;
        let y = y1 + (y2 - y1) * t;
        sink.move_to(python_round(x) as i32, python_round(y) as i32);
        if !opts.no_delay {
            let floor = opts.min_delay.max(Duration::from_secs_f64(0.003));
            let ceiling = opts.max_delay.max(floor);
            cancel.sleep(random_duration(floor, ceiling))?;
        }
    }
    Ok(())
}

/// Moves the cursor by `(dx, dy)` from wherever it currently is. Ports `nudge_cursor`.
pub fn nudge_cursor(sink: &mut dyn InputSink, dx: i32, dy: i32) {
    let (x, y) = sink.cursor_position();
    sink.move_to(x + dx, y + dy);
}

/// A duration drawn uniformly from `[floor, ceiling]`. Ports `random.uniform(sleep_floor, sleep_ceiling)`.
fn random_duration(floor: Duration, ceiling: Duration) -> Duration {
    if ceiling <= floor {
        return floor;
    }
    floor + (ceiling - floor).mul_f64(jitter::unit_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::InputEvent;

    #[derive(Default)]
    struct RecordingSink {
        events: Vec<InputEvent>,
        cursor: (i32, i32),
    }

    impl InputSink for RecordingSink {
        fn move_to(&mut self, x: i32, y: i32) {
            self.events.push(InputEvent::MoveTo { x, y });
            self.cursor = (x, y);
        }
        fn mouse_down(&mut self) {
            self.events.push(InputEvent::MouseDown);
        }
        fn mouse_up(&mut self) {
            self.events.push(InputEvent::MouseUp);
        }
        fn key_event(&mut self, vk: u16, key_up: bool) {
            self.events.push(InputEvent::Key { vk, key_up });
        }
        fn cursor_position(&self) -> (i32, i32) {
            self.cursor
        }
    }

    #[test]
    fn smooth_move_visits_exactly_steps_points_ending_on_target() {
        let mut sink = RecordingSink::default();
        let cancel = Cancel::new();
        let opts = SmoothMoveOptions { steps: 4, no_delay: true, ..Default::default() };
        move_mouse_smooth(&mut sink, &cancel, (0.0, 0.0), (100.0, 200.0), opts).unwrap();
        assert_eq!(sink.events.len(), 4);
        assert_eq!(sink.events[0], InputEvent::MoveTo { x: 25, y: 50 });
        assert_eq!(sink.events[3], InputEvent::MoveTo { x: 100, y: 200 });
    }

    #[test]
    fn smooth_move_stops_immediately_once_cancelled() {
        let mut sink = RecordingSink::default();
        let cancel = Cancel::new();
        cancel.cancel();
        let err = move_mouse_smooth(&mut sink, &cancel, (0.0, 0.0), (10.0, 10.0), SmoothMoveOptions::default()).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
        assert!(sink.events.is_empty());
    }

    #[test]
    fn nudge_cursor_moves_relative_to_current_position() {
        let mut sink = RecordingSink { cursor: (500, 500), ..Default::default() };
        nudge_cursor(&mut sink, 15, -5);
        assert_eq!(sink.events, vec![InputEvent::MoveTo { x: 515, y: 495 }]);
    }
}
