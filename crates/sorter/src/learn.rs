//! Sort learning: how the player likes their stash sorted, learned locally from how they correct
//! or accept the sorter's placements.
//!
//! Nothing here is ever sent anywhere. The Python original (`sort_sync_service.py`) once synced
//! this data to a server; that sync target is gone, and training now only ever reads the local
//! event store on a timer (see [`scheduler`]).
//!
//! Ported from DnDTools' `sort_model.py`, `sort_learning.py`, `sort_feedback.py`,
//! `sort_event_store.py` and `sort_sync_service.py`. The biggest intentional difference from the
//! Python is the model itself: this port hand-rolls weighted logistic regression instead of
//! depending on scikit-learn, and does not implement scikit-learn's gradient-boosted-tree tier.
//! See the doc comment on [`model`] for the full rationale.

mod event_store;
mod feedback;
mod features;
mod learning;
mod logistic;
mod model;
mod scheduler;

pub use event_store::{EventKind, SortEventStore, StoreError, StoredEvent};
pub use feedback::{SortFeedbackHandle, SortObserver, SortSummary};
pub use features::{item_features, rarity_rank, ItemFeatureInputs, PlacedItem};
pub use learning::{ItemPosition, SortLearningManager};
pub use model::{
    validate_model_payload, ModelPayload, SortAdaptiveModel, ITEM_FEATURE_NAMES,
    MODEL_SCHEMA_VERSION, MOVE_FEATURE_NAMES, RISK_FEATURE_NAMES,
};
pub use scheduler::{TrainOutcome, TrainScheduler};

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

/// A labeled training example, built from an event's stored feature snapshot.
///
/// Mirrors the `{"features": ..., "label": ..., "weight": ...}` event dicts that
/// `sort_model.py`'s `train_risk` / `train_items` accept. `label`'s meaning depends on the head
/// being trained: for risk, `true` means the session failed; for item placement, `true` means the
/// placement was good.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainingSample {
    pub features: HashMap<String, f64>,
    pub label: bool,
    pub weight: f64,
}

/// What [`SortLearningManager`] and [`SortObserver`] need from a model.
///
/// `SortAdaptiveModel` implements this for real use; tests substitute a stub, the same role
/// Python's tests fill with a `FakeModel` duck-typed object.
pub trait ModelPredictor: Send + Sync {
    /// Probability (0-1) that a sort session with these features will fail.
    fn predict_risk(&self, features: &HashMap<String, f64>) -> f64;
    /// Score for an (item, slot) candidate, higher is better. `None` when no model is trained yet.
    fn score_item_slot(&self, features: &HashMap<String, f64>) -> Option<f64>;
    /// Translate a risk score into a recommended workspace-buffer cell count.
    fn recommended_workspace_cells(&self, risk: Option<f64>, base_min: u32, max_extra: u32) -> u32;
}

/// Everything about one detected user correction: the player moved an item somewhere other than
/// where the sorter planned. Bundled into one struct (rather than five loose parameters) purely to
/// keep [`EventSink::record_user_correction`]'s signature small.
#[derive(Debug, Clone)]
pub struct UserCorrection {
    pub item_id: String,
    pub planned_features: HashMap<String, f64>,
    pub corrected_features: HashMap<String, f64>,
    pub planned_pos: (i64, i64),
    pub corrected_pos: (i64, i64),
}

/// Where [`SortLearningManager`] and [`SortObserver`] record what happened, for later training.
///
/// `SortEventStore` implements this for real use; tests substitute a stub, the same role Python's
/// tests fill with a `FakeStore` duck-typed object.
pub trait EventSink: Send + Sync {
    fn record_sort_started(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        metadata: serde_json::Value,
    ) -> Result<i64, StoreError>;

    fn record_sort_completed(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        success: bool,
        metadata: serde_json::Value,
    ) -> Result<i64, StoreError>;

    fn record_item_placement(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        success: bool,
        metadata: serde_json::Value,
    ) -> Result<i64, StoreError>;

    /// Records two events: a negative sample at the planned position and a positive sample at the
    /// user's corrected position. Returns both new row ids, `[negative_id, positive_id]`.
    fn record_user_correction(&self, session_id: &str, correction: UserCorrection) -> Result<Vec<i64>, StoreError>;

    /// Overrides the most recent `SORT_COMPLETED` sample for `session_id` with explicit user
    /// feedback. Returns `false` when no such session exists yet.
    fn apply_user_feedback(&self, session_id: &str, success: bool, note: Option<&str>) -> Result<bool, StoreError>;
}

/// Locks a mutex, recovering the data even if a prior panic poisoned it.
///
/// A poisoned lock means some earlier operation panicked while holding it; the plain data it
/// protects has no invariant that spans the panic point, so refusing to ever touch it again would
/// turn one bug into a permanently broken model or event store for the rest of the process.
/// Recovering is the safer choice for a local, best-effort learner than propagating a second panic.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Seconds since the Unix epoch, matching Python's `time.time()`. Falls back to 0.0 only if the
/// system clock is set before 1970, which does not happen on any real machine running this app.
pub(crate) fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// A short, unique-enough id for a sort session or similar local handle. It does not need to be
/// cryptographically random (nothing here ever leaves the machine); a nanosecond timestamp plus a
/// process-local counter avoids collisions within one run without adding a `uuid`/`rand` dependency.
pub(crate) fn new_local_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{n:x}")
}
