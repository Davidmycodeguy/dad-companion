//! Port of `sort_sync_service.py`'s `SortSyncService`: while sort learning is enabled, retrain
//! periodically from the local event store — nothing is ever uploaded or downloaded (that upstream
//! sync target is gone; see the crate doc comment).
//!
//! ## Difference from the Python: no background worker thread
//!
//! Python runs a dedicated daemon thread that sleeps for up to 30 minutes (`TRAIN_INTERVAL`) and
//! wakes early when `trigger_sync(immediate=True)` is called (e.g. right after the setting is
//! switched on). Per this port's brief, [`TrainScheduler`] has no thread or timer of its own: it is
//! a small state machine, and [`TrainScheduler::train_if_due`] is meant to be called from whatever
//! periodic tick the embedding app already has (a Tauri timer, an event-loop interval, ...). It
//! takes the actual "fetch fresh data and fit both heads" work as a closure rather than calling a
//! concrete `SortAdaptiveModel` / `SortEventStore` directly, so this module stays decoupled from
//! both and is trivially testable with a closure that just records that it ran.

/// How often to retrain while enabled, matching Python's `TRAIN_INTERVAL = 60 * 30`.
pub const TRAIN_INTERVAL_SECS: f64 = 60.0 * 30.0;

/// What a [`TrainScheduler::train_if_due`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainOutcome {
    /// Learning is switched off; the closure was not called.
    Disabled,
    /// Enabled, but neither an immediate retrain was pending nor has the interval elapsed; the
    /// closure was not called.
    NotDue,
    /// The closure was called.
    Trained,
}

/// A small, thread-free state machine deciding when to retrain. See the module doc comment for why
/// it has no worker thread of its own, unlike its Python counterpart.
pub struct TrainScheduler {
    enabled: bool,
    last_trained_at: Option<f64>,
    due_immediately: bool,
}

impl TrainScheduler {
    /// `enabled` mirrors the persisted `sortFeedbackSyncEnabled` setting at startup. Starting
    /// enabled arms an immediate retrain on the very first `train_if_due` call, the same "train
    /// right away" behavior Python's `start()` gets from calling `trigger_sync(immediate=True)`
    /// itself right after `_ensure_worker()`.
    pub fn new(enabled: bool) -> Self {
        TrainScheduler { enabled, last_trained_at: None, due_immediately: enabled }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Mirrors `sort_sync_service.py`'s `apply_settings`: updates the enabled flag, and arms an
    /// immediate retrain when learning was off and is now being turned on — matching Python's
    /// "switched on; training now" plus its own `trigger_sync(immediate=True)` call. Turning
    /// learning off does not cancel a retrain already in progress in the caller's own closure; it
    /// only stops future `train_if_due` calls from starting new ones.
    pub fn set_enabled(&mut self, enabled: bool) {
        let was_enabled = self.enabled;
        self.enabled = enabled;
        if enabled && !was_enabled {
            self.due_immediately = true;
        }
    }

    /// Arms an immediate retrain regardless of the interval, the next time `train_if_due` runs
    /// while enabled. Mirrors Python's `trigger_sync()`.
    pub fn request_immediate_retrain(&mut self) {
        self.due_immediately = true;
    }

    /// Calls `train` if due: either an immediate retrain is pending, or the model has never
    /// trained, or at least [`TRAIN_INTERVAL_SECS`] have passed since the last time this returned
    /// [`TrainOutcome::Trained`]. Does nothing while disabled. `now` is seconds since the Unix
    /// epoch (see [`super::now_secs`]) — passed in rather than read internally so callers can test
    /// the interval logic without a real clock.
    pub fn train_if_due(&mut self, now: f64, train: impl FnOnce()) -> TrainOutcome {
        if !self.enabled {
            return TrainOutcome::Disabled;
        }
        let due = self.due_immediately
            || match self.last_trained_at {
                None => true,
                Some(last) => now - last >= TRAIN_INTERVAL_SECS,
            };
        if !due {
            return TrainOutcome::NotDue;
        }
        train();
        self.last_trained_at = Some(now);
        self.due_immediately = false;
        TrainOutcome::Trained
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_scheduler_never_trains() {
        // Arrange
        let mut scheduler = TrainScheduler::new(false);

        // Act
        let outcome = scheduler.train_if_due(0.0, || panic!("must not train while disabled"));

        // Assert
        assert_eq!(outcome, TrainOutcome::Disabled);
    }

    #[test]
    fn starting_enabled_trains_immediately_on_the_first_call() {
        // Arrange
        let mut scheduler = TrainScheduler::new(true);
        let mut trained = false;

        // Act
        let outcome = scheduler.train_if_due(1_000.0, || trained = true);

        // Assert
        assert_eq!(outcome, TrainOutcome::Trained);
        assert!(trained);
    }

    #[test]
    fn does_not_retrain_again_before_the_interval_elapses() {
        // Arrange
        let mut scheduler = TrainScheduler::new(true);
        scheduler.train_if_due(1_000.0, || {});

        // Act
        let outcome = scheduler.train_if_due(1_000.0 + TRAIN_INTERVAL_SECS - 1.0, || panic!("too soon"));

        // Assert
        assert_eq!(outcome, TrainOutcome::NotDue);
    }

    #[test]
    fn retrains_once_the_interval_has_fully_elapsed() {
        // Arrange
        let mut scheduler = TrainScheduler::new(true);
        scheduler.train_if_due(1_000.0, || {});

        // Act
        let outcome = scheduler.train_if_due(1_000.0 + TRAIN_INTERVAL_SECS, || {});

        // Assert
        assert_eq!(outcome, TrainOutcome::Trained);
    }

    #[test]
    fn turning_learning_on_after_it_was_off_arms_an_immediate_retrain() {
        // Arrange: disabled at construction, so no immediate retrain is armed yet.
        let mut scheduler = TrainScheduler::new(false);
        assert_eq!(scheduler.train_if_due(1_000.0, || panic!("disabled")), TrainOutcome::Disabled);

        // Act: flip it on, well before any interval could plausibly have elapsed.
        scheduler.set_enabled(true);
        let outcome = scheduler.train_if_due(1_000.1, || {});

        // Assert
        assert_eq!(outcome, TrainOutcome::Trained);
        assert!(scheduler.is_enabled());
    }

    #[test]
    fn re_enabling_an_already_enabled_scheduler_does_not_force_a_retrain() {
        // Arrange: enabled, and already trained once (so no immediate retrain is pending).
        let mut scheduler = TrainScheduler::new(true);
        scheduler.train_if_due(1_000.0, || {});

        // Act: "turning on" a scheduler that was already on must not re-arm an immediate retrain.
        scheduler.set_enabled(true);
        let outcome = scheduler.train_if_due(1_000.1, || panic!("should still be waiting for the interval"));

        // Assert
        assert_eq!(outcome, TrainOutcome::NotDue);
    }

    #[test]
    fn turning_learning_off_stops_further_training() {
        // Arrange
        let mut scheduler = TrainScheduler::new(true);
        scheduler.train_if_due(1_000.0, || {});

        // Act
        scheduler.set_enabled(false);
        let outcome = scheduler.train_if_due(1_000.0 + TRAIN_INTERVAL_SECS, || panic!("must not train while disabled"));

        // Assert
        assert_eq!(outcome, TrainOutcome::Disabled);
        assert!(!scheduler.is_enabled());
    }
}
