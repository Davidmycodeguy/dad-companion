//! Public-API port of `test_sort_learning_does_not_duplicate_or_guess_ambiguous_corrections`
//! (DnDTools `UI/tests/test_stash_search.py`): a single matching item is corrected exactly once,
//! and an item id matching more than one item on the board is never guessed at.
//!
//! The Python test's third scenario (a pending plan older than an hour is swept as stale) backdates
//! a private timestamp directly; that scenario is ported instead as a white-box test next to the
//! private state it pokes — see `corrections_are_recorded_once_never_guessed_and_never_stale` in
//! `crates/sorter/src/learn/learning.rs`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sorter::learn::{EventSink, ItemPosition, ModelPredictor, SortLearningManager, StoreError, UserCorrection};

struct FakeModel;

impl ModelPredictor for FakeModel {
    fn predict_risk(&self, _features: &HashMap<String, f64>) -> f64 {
        0.0
    }
    fn score_item_slot(&self, _features: &HashMap<String, f64>) -> Option<f64> {
        None
    }
    fn recommended_workspace_cells(&self, _risk: Option<f64>, base_min: u32, _max_extra: u32) -> u32 {
        base_min
    }
}

/// Captures every `record_user_correction` call, the same role Python's test gives its `FakeStore`.
/// The other `EventSink` methods are never exercised by this test, so they return harmless dummies.
#[derive(Default)]
struct FakeStore {
    correction_calls: Mutex<Vec<UserCorrection>>,
}

impl EventSink for FakeStore {
    fn record_sort_started(&self, _session_id: &str, _features: &HashMap<String, f64>, _metadata: Value) -> Result<i64, StoreError> {
        Ok(0)
    }
    fn record_sort_completed(&self, _session_id: &str, _features: &HashMap<String, f64>, _success: bool, _metadata: Value) -> Result<i64, StoreError> {
        Ok(0)
    }
    fn record_item_placement(&self, _session_id: &str, _features: &HashMap<String, f64>, _success: bool, _metadata: Value) -> Result<i64, StoreError> {
        Ok(0)
    }
    fn record_user_correction(&self, _session_id: &str, correction: UserCorrection) -> Result<Vec<i64>, StoreError> {
        self.correction_calls.lock().expect("test mutex poisoned").push(correction);
        Ok(vec![1, 2])
    }
    fn apply_user_feedback(&self, _session_id: &str, _success: bool, _note: Option<&str>) -> Result<bool, StoreError> {
        Ok(false)
    }
}

fn features(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[test]
fn a_single_matching_item_is_corrected_once_and_never_repeated() {
    // Arrange
    let store = Arc::new(FakeStore::default());
    let manager = SortLearningManager::new(Arc::new(FakeModel), store.clone());
    manager.register_pending_plan(
        "session",
        HashMap::from([("potion".to_string(), (1, 1))]),
        HashMap::from([("potion".to_string(), features(&[("width", 1.0)]))]),
    );
    let moved = [ItemPosition { item_id: "potion", x: 2, y: 2 }];

    // Act
    let first_pass = manager.check_corrections(&moved);
    let second_pass = manager.check_corrections(&moved);

    // Assert: recorded once; checking again against the now-current position finds nothing new.
    assert_eq!(first_pass, 1);
    assert_eq!(second_pass, 0);
    assert_eq!(store.correction_calls.lock().expect("test mutex poisoned").len(), 1);
}

#[test]
fn an_item_id_matching_more_than_one_item_is_never_guessed_at() {
    // Arrange: two copies of "potion" are on the board, so it is ambiguous which one the plan meant.
    let store = Arc::new(FakeStore::default());
    let manager = SortLearningManager::new(Arc::new(FakeModel), store.clone());
    manager.register_pending_plan(
        "session",
        HashMap::from([("potion".to_string(), (1, 1))]),
        HashMap::from([("potion".to_string(), features(&[("width", 1.0)]))]),
    );
    let items = [ItemPosition { item_id: "potion", x: 1, y: 1 }, ItemPosition { item_id: "potion", x: 5, y: 5 }];

    // Act
    let corrections = manager.check_corrections(&items);

    // Assert
    assert_eq!(corrections, 0);
    assert!(store.correction_calls.lock().expect("test mutex poisoned").is_empty());
}
