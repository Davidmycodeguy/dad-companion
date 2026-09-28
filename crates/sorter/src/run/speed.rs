//! Drag pacing that slows down after an unconfirmed move and eases back after a run of confirmed
//! ones. Port of `sort.py`'s `AdaptiveSpeedController`, which Python only fed into learning
//! features; here the delay it computes is the one the drags actually use, and it never goes
//! faster than the player's chosen speed.

use std::time::Duration;

/// Longest delay the controller slows down to. Python: `MAX_DELAY = 0.5`.
const MAX_DELAY: Duration = Duration::from_millis(500);
/// Slowing down from "instant" (no delay) starts here, since 1.5 x 0 is still 0.
const SLOWDOWN_FLOOR: Duration = Duration::from_millis(50);
/// Python: `SLOWDOWN_FACTOR = 1.5`, `SPEEDUP_FACTOR = 0.9`, `SPEEDUP_THRESHOLD = 5`.
const SLOWDOWN_FACTOR: f64 = 1.5;
const SPEEDUP_FACTOR: f64 = 0.9;
const SPEEDUP_STREAK: u32 = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct AdaptiveSpeed {
    base: Duration,
    current: Duration,
    streak: u32,
}

impl AdaptiveSpeed {
    /// Starts at `base` (the player's chosen drag delay), capped at the slowest delay.
    pub fn new(base: Duration) -> Self {
        let base = base.min(MAX_DELAY);
        AdaptiveSpeed { base, current: base, streak: 0 }
    }

    /// The delay the next drag should use.
    pub fn delay(&self) -> Duration {
        self.current
    }

    /// Confirmed moves in a row.
    pub fn streak(&self) -> u32 {
        self.streak
    }

    /// Records one attempt: an unconfirmed one slows down by half again; once five in a row are
    /// confirmed, each further one eases 10% back toward the base delay (Python's rule exactly).
    pub fn record(&mut self, verified: bool) {
        if verified {
            self.streak += 1;
            if self.streak >= SPEEDUP_STREAK {
                self.current = self.current.mul_f64(SPEEDUP_FACTOR).max(self.base);
            }
        } else {
            self.streak = 0;
            self.current = self.current.max(SLOWDOWN_FLOOR).mul_f64(SLOWDOWN_FACTOR).min(MAX_DELAY);
        }
    }
}
