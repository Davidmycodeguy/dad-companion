//! Public-API port of `test_sort_learning_opt_in_starts_the_worker_and_trains_locally`
//! (DnDTools `UI/tests/test_stash_search.py`): turning sort learning on trains right away, and
//! turning it back off stops further training.
//!
//! Python's `SortSyncService` runs its own worker thread, and the test observes it indirectly by
//! substituting `_ensure_worker` / `trigger_sync`. This port has no worker thread at all (see the
//! module doc comment on `crates/sorter/src/learn/scheduler.rs`), so the equivalent behavior is
//! observed directly through `TrainScheduler::train_if_due`'s return value.

use sorter::learn::{TrainOutcome, TrainScheduler};

const THIRTY_MINUTES: f64 = 60.0 * 30.0;

#[test]
fn turning_learning_on_trains_immediately_and_turning_it_off_stops_training() {
    // Arrange: starts disabled, matching the Python test's `sortFeedbackSyncEnabled: False`.
    let mut scheduler = TrainScheduler::new(false);
    assert!(!scheduler.is_enabled());

    // Act: switching it on, matching `apply_settings({"sortFeedbackSyncEnabled": True})`.
    scheduler.set_enabled(true);

    // Assert: it trains on the very next call, with no 30-minute interval to wait out — the
    // behavior Python gets from `_ensure_worker()` followed by `trigger_sync(immediate=True)`.
    assert!(scheduler.is_enabled());
    let mut trained = false;
    let outcome = scheduler.train_if_due(1_000.0, || trained = true);
    assert_eq!(outcome, TrainOutcome::Trained);
    assert!(trained);

    // Act: switching it back off, matching `apply_settings({"sortFeedbackSyncEnabled": False})`.
    scheduler.set_enabled(false);

    // Assert: no more training happens, even once the interval has fully elapsed.
    assert!(!scheduler.is_enabled());
    let outcome = scheduler.train_if_due(1_000.0 + THIRTY_MINUTES, || panic!("must not train while disabled"));
    assert_eq!(outcome, TrainOutcome::Disabled);
}
