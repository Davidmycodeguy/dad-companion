//! Port of `test_manual_sort_feedback_updates_real_session_sample_and_requeues_sync`
//! (DnDTools `UI/tests/test_stash_search.py`): explicit user feedback overrides a real completed
//! session's own sample in place (not a new, feature-less row) and re-queues it, and the app's
//! retrain hook fires exactly once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use sorter::learn::{ModelPredictor, SortEventStore, SortObserver};

struct FakeModel;

impl ModelPredictor for FakeModel {
    fn predict_risk(&self, _features: &HashMap<String, f64>) -> f64 {
        0.0
    }
    fn score_item_slot(&self, _features: &HashMap<String, f64>) -> Option<f64> {
        None
    }
    fn recommended_workspace_cells(&self, _risk: Option<f64>, _base_min: u32, _max_extra: u32) -> u32 {
        6
    }
}

fn features(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[test]
fn manual_feedback_updates_the_real_session_sample_and_requeues_it_for_sync() {
    // Arrange: a real event store with one already-synced completed session.
    let dir = tempfile::tempdir().expect("create temp dir for test store");
    let store = Arc::new(SortEventStore::new(dir.path()).expect("open event store"));
    let event_id = store
        .record_sort_completed(
            "session-1",
            &features(&[("stash_fill_ratio", 0.75), ("inventory_free_ratio", 0.25)]),
            true,
            serde_json::json!({ "plan_size": 7 }),
        )
        .expect("record sort completed");
    store.mark_synced(&[event_id]).expect("mark synced");

    let scheduled = Arc::new(Mutex::new(Vec::new()));
    let scheduled_writer = Arc::clone(&scheduled);
    let observer = SortObserver::new(Arc::new(FakeModel), store.clone(), move || {
        scheduled_writer.lock().expect("test mutex poisoned").push(true);
    });

    // Act
    let accepted = observer.record_user_feedback("session-1", false, Some("items got stuck"));

    // Assert: the row was updated in place, not duplicated.
    assert!(accepted);
    assert_eq!(store.count_events().expect("count events"), 1);

    let events = store.get_unsynced_events(10).expect("query unsynced events");
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.id, event_id);
    assert_eq!(event.features.get("stash_fill_ratio"), Some(&0.75));
    assert_eq!(event.features.get("inventory_free_ratio"), Some(&0.25));
    assert_eq!(event.label, Some(true), "false success must relabel the row as a failure (1)");
    assert_eq!(event.metadata["user_feedback"], Value::Bool(true));
    assert_eq!(event.metadata["user_success"], Value::Bool(false));
    assert_eq!(event.metadata["user_note"], Value::String("items got stuck".to_string()));
    assert_eq!(*scheduled.lock().expect("test mutex poisoned"), vec![true]);

    // Act / Assert: feedback for a session that never completed is rejected outright.
    assert!(!observer.record_user_feedback("missing-session", true, None));

    store.close();
}
