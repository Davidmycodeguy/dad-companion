//! Port of `sort_learning.py`'s `SortLearningManager`: scores item-slot candidates via the model
//! and detects when the player manually corrects the sorter's plan, turning each correction into a
//! weighted negative sample (the planned position) and a weighted positive sample (the player's
//! chosen position).
//!
//! Python's `record_priority_sample` is kept only as a documented no-op, for callers written
//! against an older version of that class; this port has no such legacy caller, so it is omitted
//! rather than translated. Its replacement, `record_priority_outcome`, is ported below.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::{lock, now_secs, EventSink, ModelPredictor, UserCorrection};

/// How long a registered plan stays eligible for correction detection before it is dropped as
/// stale. Matches `sort_learning.py`'s hard-coded one hour.
const PENDING_PLAN_TTL_SECS: f64 = 3600.0;

/// One item's real, current position, as `check_corrections` should see it. Deliberately minimal
/// (not `state::OwnedItem` itself) so this module does not need to know anything about the game's
/// item shape beyond an id and a cell — the caller (whatever reads the live stash) adapts to this.
#[derive(Debug, Clone, Copy)]
pub struct ItemPosition<'a> {
    pub item_id: &'a str,
    pub x: i64,
    pub y: i64,
}

/// The sorter's plan for one session, as last reconciled against reality. Positions and features
/// are advanced in place after each recorded correction (see `check_corrections`), so replaying the
/// same manual move twice never records it twice.
struct PendingPlan {
    registered_at: f64,
    positions: HashMap<String, (i64, i64)>,
    features: HashMap<String, HashMap<String, f64>>,
}

/// `(session_id, positions, features)` snapshotted out of a [`PendingPlan`] — see
/// `SortLearningManager::latest_pending_snapshot`.
type PendingSnapshot = (String, HashMap<String, (i64, i64)>, HashMap<String, HashMap<String, f64>>);

/// Scores items for the planner and detects post-sort user corrections, recording both to an
/// [`EventSink`] for later training.
pub struct SortLearningManager {
    model: Arc<dyn ModelPredictor>,
    event_store: Arc<dyn EventSink>,
    pending: Mutex<HashMap<String, PendingPlan>>,
}

impl SortLearningManager {
    pub fn new(model: Arc<dyn ModelPredictor>, event_store: Arc<dyn EventSink>) -> Self {
        SortLearningManager { model, event_store, pending: Mutex::new(HashMap::new()) }
    }

    /// Scores an item-slot pair for the planner. `None` when no item model is available yet, in
    /// which case the caller should fall back to its own heuristics.
    pub fn score_item(&self, features: &HashMap<String, f64>) -> Option<f64> {
        self.model.score_item_slot(features)
    }

    /// Records the outcome of an item placement. A no-op when `session_id` is `None`, since there
    /// is then nothing to associate the sample with — matches Python's early `if not session_id`
    /// return. Recording failures are swallowed the same way Python's `except Exception` around
    /// this call is: one lost training sample must never fail a sort.
    pub fn record_priority_outcome(&self, session_id: Option<&str>, features: &HashMap<String, f64>, success: bool, metadata: Value) {
        let Some(session_id) = session_id else { return };
        let _ = self.event_store.record_item_placement(session_id, features, success, metadata);
    }

    /// Registers the sorter's plan for `session_id` so a later [`Self::check_corrections`] call can
    /// detect when the player moves something the sorter placed. Also sweeps any plan older than
    /// one hour. A no-op for an empty session id or an empty plan, matching Python.
    pub fn register_pending_plan(
        &self,
        session_id: &str,
        plan: HashMap<String, (i64, i64)>,
        features: HashMap<String, HashMap<String, f64>>,
    ) {
        if session_id.is_empty() || plan.is_empty() {
            return;
        }
        let mut pending = lock(&self.pending);
        let now = now_secs();
        pending.retain(|_, data| now - data.registered_at < PENDING_PLAN_TTL_SECS);
        pending.insert(session_id.to_string(), PendingPlan { registered_at: now, positions: plan, features });
    }

    /// Compares `items`' real positions against the most recently registered pending plan (only the
    /// single most recent one — an older pending plan for a different session is left untouched,
    /// matching Python). Any item whose real position differs from its planned one is a correction:
    /// it writes a weighted negative sample at the planned position and a weighted positive sample
    /// at the corrected one, then advances the plan's baseline so the same manual move is never
    /// recorded twice.
    ///
    /// An item id matching more than one of `items` is skipped entirely: the plan is keyed by
    /// design item id, so several copies of the same item make "which one moved" ambiguous, and
    /// guessing would poison training with a fabricated sample.
    ///
    /// Returns how many corrections were found and recorded.
    pub fn check_corrections(&self, items: &[ItemPosition]) -> usize {
        let Some((session_id, planned_positions, planned_features)) = self.latest_pending_snapshot() else {
            return 0;
        };

        let mut items_by_id: HashMap<&str, Vec<&ItemPosition>> = HashMap::new();
        for item in items {
            if planned_positions.contains_key(item.item_id) {
                items_by_id.entry(item.item_id).or_default().push(item);
            }
        }

        let mut corrections_found = 0;
        for (item_id, matching) in items_by_id {
            if matching.len() != 1 {
                continue; // ambiguous: several copies of this item are on the board right now.
            }
            let item = matching[0];
            let Some(&(planned_x, planned_y)) = planned_positions.get(item_id) else { continue };
            if item.x == planned_x && item.y == planned_y {
                continue; // still where the sorter put it.
            }

            let original_features = planned_features.get(item_id).cloned().unwrap_or_default();
            let mut planned_feat = original_features.clone();
            planned_feat.insert("slot_x".to_string(), planned_x as f64);
            planned_feat.insert("slot_y".to_string(), planned_y as f64);
            let mut corrected_feat = original_features;
            corrected_feat.insert("slot_x".to_string(), item.x as f64);
            corrected_feat.insert("slot_y".to_string(), item.y as f64);

            let correction = UserCorrection {
                item_id: item_id.to_string(),
                planned_features: planned_feat,
                corrected_features: corrected_feat.clone(),
                planned_pos: (planned_x, planned_y),
                corrected_pos: (item.x, item.y),
            };

            if self.event_store.record_user_correction(&session_id, correction).is_ok() {
                corrections_found += 1;
                self.advance_pending_baseline(&session_id, item_id, (item.x, item.y), corrected_feat);
            }
        }
        corrections_found
    }

    /// Sweeps stale plans, then snapshots the positions/features of whichever remaining plan was
    /// registered most recently. Cloned out from under the lock so the (potentially slow) event
    /// store call in `check_corrections` never runs while holding it.
    fn latest_pending_snapshot(&self) -> Option<PendingSnapshot> {
        let mut pending = lock(&self.pending);
        let now = now_secs();
        pending.retain(|_, data| now - data.registered_at < PENDING_PLAN_TTL_SECS);

        let session_id = pending.iter().max_by(|a, b| a.1.registered_at.total_cmp(&b.1.registered_at))?.0.clone();
        let plan = pending.get(&session_id)?;
        Some((session_id, plan.positions.clone(), plan.features.clone()))
    }

    /// After a correction is recorded, moves the pending plan's belief for `item_id` to where the
    /// player actually put it, so re-running `check_corrections` against unchanged input never
    /// records the same manual move a second time.
    fn advance_pending_baseline(&self, session_id: &str, item_id: &str, corrected_pos: (i64, i64), corrected_features: HashMap<String, f64>) {
        let mut pending = lock(&self.pending);
        if let Some(plan) = pending.get_mut(session_id) {
            plan.positions.insert(item_id.to_string(), corrected_pos);
            plan.features.insert(item_id.to_string(), corrected_features);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[derive(Default)]
    struct FakeStore {
        calls: Mutex<Vec<UserCorrection>>,
    }
    impl EventSink for FakeStore {
        fn record_sort_started(&self, _session_id: &str, _features: &HashMap<String, f64>, _metadata: Value) -> Result<i64, super::super::StoreError> {
            Ok(0)
        }
        fn record_sort_completed(&self, _session_id: &str, _features: &HashMap<String, f64>, _success: bool, _metadata: Value) -> Result<i64, super::super::StoreError> {
            Ok(0)
        }
        fn record_item_placement(&self, _session_id: &str, _features: &HashMap<String, f64>, _success: bool, _metadata: Value) -> Result<i64, super::super::StoreError> {
            Ok(0)
        }
        fn record_user_correction(&self, _session_id: &str, correction: UserCorrection) -> Result<Vec<i64>, super::super::StoreError> {
            lock(&self.calls).push(correction);
            Ok(vec![1, 2])
        }
        fn apply_user_feedback(&self, _session_id: &str, _success: bool, _note: Option<&str>) -> Result<bool, super::super::StoreError> {
            Ok(false)
        }
    }

    fn features(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    // Port of `test_sort_learning_does_not_duplicate_or_guess_ambiguous_corrections`
    // (DnDTools `UI/tests/test_stash_search.py`). Kept as one white-box test, like the Python
    // original, because its third scenario backdates a pending plan's timestamp directly — the
    // same access Python's test gets by poking `_pending_plans` — which only this module's own
    // tests can do. The first two scenarios are re-verified from outside in
    // `tests/learn_corrections.rs`, using only public API.
    #[test]
    fn corrections_are_recorded_once_never_guessed_and_never_stale() {
        // Scenario 1: a single matching item is corrected once, then the baseline advances so
        // checking again against the same (now-current) position finds nothing new.
        let store = Arc::new(FakeStore::default());
        let manager = SortLearningManager::new(Arc::new(FakeModel), store.clone());
        manager.register_pending_plan(
            "session",
            HashMap::from([("potion".to_string(), (1, 1))]),
            HashMap::from([("potion".to_string(), features(&[("width", 1.0)]))]),
        );
        let moved = [ItemPosition { item_id: "potion", x: 2, y: 2 }];
        assert_eq!(manager.check_corrections(&moved), 1);
        assert_eq!(lock(&store.calls).len(), 1);
        assert_eq!(manager.check_corrections(&moved), 0);
        assert_eq!(lock(&store.calls).len(), 1);

        // Scenario 2: two copies of the same planned item id on the board make "which one moved"
        // ambiguous, so nothing is recorded at all.
        let ambiguous_store = Arc::new(FakeStore::default());
        let ambiguous = SortLearningManager::new(Arc::new(FakeModel), ambiguous_store.clone());
        ambiguous.register_pending_plan(
            "session",
            HashMap::from([("potion".to_string(), (1, 1))]),
            HashMap::from([("potion".to_string(), features(&[("width", 1.0)]))]),
        );
        let items = [ItemPosition { item_id: "potion", x: 1, y: 1 }, ItemPosition { item_id: "potion", x: 5, y: 5 }];
        assert_eq!(ambiguous.check_corrections(&items), 0);
        assert!(lock(&ambiguous_store.calls).is_empty());

        // Scenario 3: a pending plan registered over an hour ago is swept as stale before it is
        // ever compared against, so it records nothing even for an otherwise-valid correction.
        let stale_store = Arc::new(FakeStore::default());
        let stale = SortLearningManager::new(Arc::new(FakeModel), stale_store.clone());
        stale.register_pending_plan(
            "session",
            HashMap::from([("potion".to_string(), (1, 1))]),
            HashMap::from([("potion".to_string(), features(&[("width", 1.0)]))]),
        );
        lock(&stale.pending).get_mut("session").expect("just registered").registered_at -= 7200.0;
        assert_eq!(stale.check_corrections(&moved), 0);
        assert!(lock(&stale_store.calls).is_empty());
    }
}
