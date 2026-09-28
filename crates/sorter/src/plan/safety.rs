//! Sort-safety decision logic: when to cancel an in-progress sort because the game window lost
//! focus or the player's mouse moved unexpectedly. Port of `sort_safety.py`'s
//! `SortSafetyMonitor` — its OS polling (`GetForegroundWindow`/`GetCursorPos` on a background
//! thread) belongs to the mouse-execution layer built elsewhere; this is the pure decision state
//! machine that layer should drive with the positions and timestamps it observes.

use std::time::{Duration, Instant};

/// Why a sort was cancelled by safety monitoring. Python: the string `SortSafetyMonitor.reason`
/// ends up holding (`"game_window_unfocused"` / `"mouse_interference"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The game window was not in the foreground for longer than the grace period.
    GameWindowUnfocused,
    /// The cursor moved further than the threshold on `MOUSE_DEVIATION_STRIKES` consecutive
    /// checkpoints.
    MouseInterference,
}

/// Detects the player fighting the mouse or alt-tabbing away mid-sort. Construct one per sort;
/// feed it positions and focus polls as the execution layer observes them. Python:
/// `SortSafetyMonitor` (minus its `cancel_event` gate, which is the caller's own concern, and its
/// background thread, which is how the caller should be driving `poll_focus`).
#[derive(Debug, Clone)]
pub struct SortSafetyMonitor {
    expected_pos: Option<(i32, i32)>,
    deviation_strikes: u32,
    focus_lost_since: Option<Instant>,
    reason: Option<CancelReason>,
}

impl SortSafetyMonitor {
    /// How long the game may lose focus before the sort is cancelled. Short enough to react
    /// quickly, long enough to survive a brief taskbar flash or overlay tooltip.
    pub const FOCUS_LOSS_GRACE: Duration = Duration::from_millis(600);
    /// Pixels the cursor must move (in either axis) from the expected position to count as a
    /// deviation strike. At 1080p the default stash cell jump is ~40px, so 120px is ~3 cells —
    /// above normal macro jitter but easy to hit with a deliberate hand move.
    pub const MOUSE_DEVIATION_THRESHOLD_PX: i32 = 120;
    /// Consecutive deviation checkpoints required before cancelling. Checkpoints happen between
    /// item moves, so this means the player must visibly fight the cursor for several items
    /// running before the sort aborts — virtually impossible to trigger by accident.
    pub const MOUSE_DEVIATION_STRIKES: u32 = 3;

    pub fn new() -> Self {
        Self { expected_pos: None, deviation_strikes: 0, focus_lost_since: None, reason: None }
    }

    /// Set once cancellation triggers; which of `checkpoint`/`poll_focus` returned `false`.
    pub fn reason(&self) -> Option<CancelReason> {
        self.reason
    }

    /// Records `current` and compares it with the position from the previous checkpoint (or the
    /// last `snapshot_position`). Returns `false` once the deviation threshold has been exceeded
    /// on `MOUSE_DEVIATION_STRIKES` consecutive checkpoints; a single deviation only counts a
    /// strike, so one bumped desk or small nudge never aborts a sort by itself. Port of
    /// `SortSafetyMonitor.checkpoint`.
    pub fn checkpoint(&mut self, current: (i32, i32)) -> bool {
        let Some((expected_x, expected_y)) = self.expected_pos.replace(current) else {
            return true; // first checkpoint has nothing to compare against yet
        };
        let deviation = (current.0 - expected_x).abs().max((current.1 - expected_y).abs());
        if deviation > Self::MOUSE_DEVIATION_THRESHOLD_PX {
            self.deviation_strikes += 1;
            if self.deviation_strikes >= Self::MOUSE_DEVIATION_STRIKES {
                self.reason = Some(CancelReason::MouseInterference);
                return false;
            }
        } else {
            self.deviation_strikes = 0;
        }
        true
    }

    /// Records `current` as the new expected baseline, e.g. immediately after a macro move
    /// completes so the next `checkpoint` compares against the correct resting position. Port of
    /// `snapshot_position`.
    pub fn snapshot_position(&mut self, current: (i32, i32)) {
        self.expected_pos = Some(current);
        self.deviation_strikes = 0;
    }

    /// Feeds one focus poll. `now`/`game_focused` come from whatever the execution layer uses to
    /// read the foreground window; returns `false` once the window has been unfocused for longer
    /// than `FOCUS_LOSS_GRACE`. Port of `SortSafetyMonitor._focus_loop`'s per-tick decision (the
    /// polling loop itself belongs to that layer).
    pub fn poll_focus(&mut self, now: Instant, game_focused: bool) -> bool {
        if game_focused {
            self.focus_lost_since = None;
            return true;
        }
        match self.focus_lost_since {
            None => {
                self.focus_lost_since = Some(now);
                true
            }
            Some(since) if now.saturating_duration_since(since) >= Self::FOCUS_LOSS_GRACE => {
                self.reason = Some(CancelReason::GameWindowUnfocused);
                false
            }
            Some(_) => true,
        }
    }
}

impl Default for SortSafetyMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_checkpoint_never_triggers() {
        let mut monitor = SortSafetyMonitor::new();
        assert!(monitor.checkpoint((5000, 5000)));
        assert!(monitor.reason().is_none());
    }

    #[test]
    fn a_lone_deviation_does_not_cancel() {
        let mut monitor = SortSafetyMonitor::new();
        assert!(monitor.checkpoint((0, 0)));
        assert!(monitor.checkpoint((500, 0))); // one strike
        assert!(monitor.checkpoint((500, 0))); // back to resting: strikes reset
        assert!(monitor.reason().is_none());
    }

    #[test]
    fn three_consecutive_deviations_cancel() {
        let mut monitor = SortSafetyMonitor::new();
        assert!(monitor.checkpoint((0, 0)));
        assert!(monitor.checkpoint((500, 0)));
        assert!(monitor.checkpoint((0, 500)));
        assert!(!monitor.checkpoint((500, 500)));
        assert_eq!(monitor.reason(), Some(CancelReason::MouseInterference));
    }

    #[test]
    fn snapshot_position_resets_the_baseline_and_strikes() {
        let mut monitor = SortSafetyMonitor::new();
        assert!(monitor.checkpoint((0, 0)));
        assert!(monitor.checkpoint((500, 0)));
        monitor.snapshot_position((500, 0));
        assert!(monitor.checkpoint((500, 0)));
        assert!(monitor.reason().is_none());
    }

    #[test]
    fn losing_focus_briefly_does_not_cancel() {
        let mut monitor = SortSafetyMonitor::new();
        let start = Instant::now();
        assert!(monitor.poll_focus(start, false));
        assert!(monitor.poll_focus(start + Duration::from_millis(300), false));
        assert!(monitor.poll_focus(start + Duration::from_millis(400), true)); // refocused in time
        assert!(monitor.reason().is_none());
    }

    #[test]
    fn losing_focus_past_the_grace_period_cancels() {
        let mut monitor = SortSafetyMonitor::new();
        let start = Instant::now();
        assert!(monitor.poll_focus(start, false));
        assert!(!monitor.poll_focus(start + SortSafetyMonitor::FOCUS_LOSS_GRACE, false));
        assert_eq!(monitor.reason(), Some(CancelReason::GameWindowUnfocused));
    }
}
