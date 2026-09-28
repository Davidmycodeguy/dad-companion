//! Port of `sort_feedback.py`'s `SortObserver` (and its `SortFeedbackHandle`): predicts risk at
//! sort start, records `SORT_STARTED` / `SORT_COMPLETED` events, and accepts explicit user
//! feedback on a finished session.
//!
//! ## Difference from the Python: who triggers retraining
//!
//! Python's `finalize()` and `record_user_feedback()` each spawn a background thread that pulls
//! fresh training data and retrains the risk head immediately. This port replaces that per-event
//! thread-spawn with an injected `on_retrain_needed` callback (see [`SortObserver::new`]): both
//! call sites still fire it at exactly the same two moments (a session just finished; feedback was
//! just applied), but *what* happens next — fetching data, spawning a thread, or doing nothing and
//! waiting for [`super::TrainScheduler::train_if_due`]'s regular cadence — is the embedding app's
//! choice, not this module's. This keeps `SortObserver` decoupled from a concrete
//! `SortAdaptiveModel` (it only needs the minimal [`super::ModelPredictor`] /
//! [`super::EventSink`] traits, which is what makes it possible to test with stand-ins).

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::{new_local_id, now_secs, EventSink, ModelPredictor};

fn get(map: &HashMap<String, f64>, key: &str, default: f64) -> f64 {
    map.get(key).copied().unwrap_or(default)
}

/// Turns raw session features/metrics into the `RISK_FEATURE_NAMES`-shaped ratios the risk model
/// trains and predicts on. Ported from `sort_feedback.py`'s `SortObserver._build_risk_features`,
/// formula for formula.
fn build_risk_features(features: &HashMap<String, f64>, metrics: &HashMap<String, f64>, pack_mode: bool, stack_mode: bool) -> HashMap<String, f64> {
    let stash_total = get(features, "stash_total_cells", 1.0).max(1.0);
    let stash_occupied = get(features, "stash_occupied_cells", 0.0);
    let inventory_total = get(features, "inventory_total_cells", 1.0).max(1.0);
    let inventory_free = get(features, "inventory_free_cells", 0.0);
    let plan_size = get(metrics, "plan_size", 0.0);
    let largest_area = {
        let from_metrics = metrics.get("largest_item_area").copied();
        from_metrics.unwrap_or_else(|| get(features, "largest_item_area", 0.0))
    };
    let workspace_prep_moves = get(metrics, "workspace_preparation_moves", 0.0);
    let buffered_items = get(metrics, "buffered_items", 0.0);
    let park_attempts = get(metrics, "park_attempts", 0.0);
    let workspace_attempts = get(metrics, "workspace_creation_attempts", 0.0);
    let workspace_failures = get(metrics, "workspace_creation_failures", 0.0);

    let plan_norm = plan_size.max(1.0);
    let workspace_attempts_norm = workspace_attempts.max(1.0);

    HashMap::from([
        ("stash_fill_ratio".to_string(), stash_occupied / stash_total),
        ("inventory_free_ratio".to_string(), inventory_free / inventory_total),
        ("plan_density".to_string(), plan_size / stash_total),
        ("largest_item_ratio".to_string(), largest_area / stash_total),
        ("workspace_prep_ratio".to_string(), workspace_prep_moves / plan_norm),
        ("buffer_ratio".to_string(), buffered_items / plan_norm),
        ("park_ratio".to_string(), park_attempts / plan_norm),
        ("workspace_failure_ratio".to_string(), workspace_failures / workspace_attempts_norm),
        ("pack_mode".to_string(), f64::from(pack_mode)),
        ("stack_mode".to_string(), f64::from(stack_mode)),
    ])
}

/// What [`SortFeedbackHandle::finalize`] returns: a small summary of the finished session, mirroring
/// the dict Python's `finalize()` returns.
#[derive(Debug, Clone, PartialEq)]
pub struct SortSummary {
    pub session_id: String,
    pub predicted_risk: f64,
    pub workspace_target: Option<u32>,
    pub duration_ms: u64,
    pub success: bool,
    pub cancelled: bool,
}

struct ObserverInner {
    model: Arc<dyn ModelPredictor>,
    event_store: Arc<dyn EventSink>,
    on_retrain_needed: Box<dyn Fn() + Send + Sync>,
}

/// Predicts sort-session risk and records session lifecycle events. Cheap to clone (internally an
/// `Arc`), matching the role Python's module-level singleton (`get_sort_feedback_manager()`) plays.
#[derive(Clone)]
pub struct SortObserver {
    inner: Arc<ObserverInner>,
}

impl SortObserver {
    /// `on_retrain_needed` is called right after a session completes and right after user feedback
    /// is applied — both moments the risk model just gained a fresh labeled sample worth learning
    /// from sooner than the next scheduled retrain. See the module doc comment for why this is a
    /// callback rather than a direct call into a concrete model.
    pub fn new(model: Arc<dyn ModelPredictor>, event_store: Arc<dyn EventSink>, on_retrain_needed: impl Fn() + Send + Sync + 'static) -> Self {
        SortObserver { inner: Arc::new(ObserverInner { model, event_store, on_retrain_needed: Box::new(on_retrain_needed) }) }
    }

    /// Convenience for an app that only wants [`super::TrainScheduler::train_if_due`]'s regular
    /// cadence and does not need an immediate retrain nudge after each session.
    pub fn without_immediate_retrain(model: Arc<dyn ModelPredictor>, event_store: Arc<dyn EventSink>) -> Self {
        Self::new(model, event_store, || {})
    }

    /// Starts observing one sort session: predicts its risk up front from `features` (the sorter's
    /// raw stash/inventory counts, e.g. `stash_total_cells`) and records a `SORT_STARTED` event.
    pub fn begin_session(
        &self,
        character_id: Option<String>,
        stash_id: Option<u32>,
        pack_mode: bool,
        stack_mode: bool,
        features: HashMap<String, f64>,
    ) -> SortFeedbackHandle {
        let session_id = new_local_id();
        let risk_features = build_risk_features(&features, &HashMap::new(), pack_mode, stack_mode);
        let prediction = self.inner.model.predict_risk(&risk_features);

        let mut metadata = serde_json::Map::new();
        if let Some(id) = &character_id {
            metadata.insert("character_id".to_string(), Value::String(id.clone()));
        }
        if let Some(id) = stash_id {
            metadata.insert("stash_id".to_string(), serde_json::json!(id));
        }
        metadata.insert("pack_mode".to_string(), Value::Bool(pack_mode));
        metadata.insert("stack_mode".to_string(), Value::Bool(stack_mode));
        metadata.insert("prediction".to_string(), serde_json::json!(prediction));
        let _ = self.inner.event_store.record_sort_started(&session_id, &risk_features, Value::Object(metadata));

        SortFeedbackHandle {
            observer: self.clone(),
            session_id,
            pack_mode,
            stack_mode,
            started_at: now_secs(),
            features,
            metrics: HashMap::new(),
            prediction,
        }
    }

    /// Overrides the most recent completed session's outcome with explicit user feedback (e.g. "it
    /// actually failed, items got stuck"). Returns `false` when no such session exists yet, or when
    /// the event store operation itself fails — either way there is nothing more for the caller to
    /// do, matching Python's `except Exception: return False`.
    pub fn record_user_feedback(&self, session_id: &str, success: bool, note: Option<&str>) -> bool {
        let accepted = self.inner.event_store.apply_user_feedback(session_id, success, note).unwrap_or(false);
        if accepted {
            (self.inner.on_retrain_needed)();
        }
        accepted
    }
}

/// Per-session handle handed to the sorter for the duration of one sort. Accumulates metrics as the
/// sort runs (`increment` / `set_metric` / `set_feature`), then [`Self::finalize`] records the
/// outcome. Consuming `self` in `finalize` makes calling it twice a compile error, which is a
/// stricter guarantee than Python's runtime `if self._finalized: return ...` check gives there.
pub struct SortFeedbackHandle {
    observer: SortObserver,
    session_id: String,
    pack_mode: bool,
    stack_mode: bool,
    started_at: f64,
    features: HashMap<String, f64>,
    metrics: HashMap<String, f64>,
    prediction: f64,
}

impl SortFeedbackHandle {
    /// Adds `amount` to a running counter, creating it at `0.0` first if this is the first mention.
    pub fn increment(&mut self, key: &str, amount: f64) {
        *self.metrics.entry(key.to_string()).or_insert(0.0) += amount;
    }

    pub fn set_metric(&mut self, key: &str, value: f64) {
        self.metrics.insert(key.to_string(), value);
    }

    pub fn set_feature(&mut self, key: &str, value: f64) {
        self.features.insert(key.to_string(), value);
    }

    /// The risk predicted when this session began.
    pub fn prediction(&self) -> f64 {
        self.prediction
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Translates this session's predicted risk into a recommended workspace-buffer cell count, and
    /// remembers it under the `workspace_target` feature for `finalize`'s summary.
    pub fn recommended_workspace_cells(&mut self, base_min: u32, max_extra: u32) -> u32 {
        let risk = self.prediction.clamp(0.0, 1.0);
        let workspace = self.observer.inner.model.recommended_workspace_cells(Some(risk), base_min, max_extra);
        self.features.insert("workspace_target".to_string(), f64::from(workspace));
        workspace
    }

    /// Records this session's outcome as a `SORT_COMPLETED` event and asks the app to consider an
    /// immediate retrain (see the module doc comment). Returns a small summary of the session.
    pub fn finalize(self, success: bool, cancelled: bool, failure_reason: Option<String>) -> SortSummary {
        let duration_ms = ((now_secs() - self.started_at) * 1000.0).max(0.0) as u64;
        let risk_features = build_risk_features(&self.features, &self.metrics, self.pack_mode, self.stack_mode);

        let mut metadata = serde_json::Map::new();
        for (key, value) in &self.metrics {
            metadata.insert(key.clone(), serde_json::json!(value));
        }
        metadata.insert("duration_ms".to_string(), serde_json::json!(duration_ms));
        metadata.insert("cancelled".to_string(), Value::Bool(cancelled));
        if let Some(reason) = &failure_reason {
            metadata.insert("failure_reason".to_string(), Value::String(reason.clone()));
        }

        let _ = self.observer.inner.event_store.record_sort_completed(&self.session_id, &risk_features, success, Value::Object(metadata));
        (self.observer.inner.on_retrain_needed)();

        let workspace_target = self.features.get("workspace_target").map(|value| *value as u32);
        SortSummary {
            session_id: self.session_id,
            predicted_risk: self.prediction,
            workspace_target,
            duration_ms,
            success,
            cancelled,
        }
    }
}
