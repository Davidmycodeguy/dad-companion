//! Port of `sort_model.py`'s `SortAdaptiveModel`: predicts sort-session risk, scores item-slot
//! candidates, and learns from outcomes and corrections with weighted samples.
//!
//! ## Difference from the Python: the learning algorithm
//!
//! Python tries, in order: (1) scikit-learn's `HistGradientBoostingClassifier`, (2) scikit-learn's
//! `LogisticRegression`, (3) nothing (keep the previous model). Whichever tier trains, Python
//! *always* also fits a `LogisticRegression` to get linear coefficients, because that is the only
//! shape it ever serializes to `risk_model.json` / `item_model.json` — the tree ensemble itself
//! never leaves the Python process. So even in Python, every persisted model — and therefore every
//! model this Rust port might load from an existing install, or exchange with `apply_remote_*` —
//! is a plain linear model evaluated as `sigmoid(coefficients · features + intercept)` (risk) or
//! `coefficients · features + intercept` (item score, unsquashed).
//!
//! This port implements exactly that tier: a hand-rolled weighted logistic regression (see
//! [`super::logistic`]), no external ML crate, no gradient-boosted trees. Reasons:
//!   - Exact numeric parity with scikit-learn is explicitly not required.
//!   - None of the ported Python tests exercise the tree-boosting code path (they either mock
//!     training entirely or apply a pre-built linear payload directly).
//!   - The persisted/exchanged format is linear regardless of which sklearn tier trained it, so a
//!     linear model is fully format- and behaviour-compatible for prediction and model exchange.
//!   - It keeps this crate free of heavy ML dependencies (no ndarray/BLAS) and system requirements,
//!     and makes training fully deterministic (no `random_state` to match).
//!
//! One consequence: this port has no separate "fitted estimator" distinct from the serialized
//! payload, the way Python's `_risk_estimator` / `_item_estimator` are distinct from
//! `_risk_payload` / `_item_payload`. Predicting always reads the current payload directly, so
//! applying a new payload (local training or `apply_remote_*`) takes effect for the very next
//! prediction with nothing extra to invalidate.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use serde::Serialize;
use serde_json::Value;

use super::{ModelPredictor, TrainingSample};

/// Features describing a sort *session*, used to predict whether it will fail. Ported from
/// `sort_model.py`'s `RISK_FEATURE_NAMES`; order matters — it is the vector order the linear
/// model's coefficients are indexed by.
pub const RISK_FEATURE_NAMES: &[&str] = &[
    "stash_fill_ratio",
    "inventory_free_ratio",
    "plan_density",
    "largest_item_ratio",
    "workspace_prep_ratio",
    "buffer_ratio",
    "park_ratio",
    "workspace_failure_ratio",
    "pack_mode",
    "stack_mode",
];

/// Features describing one (item, candidate slot) pair, used to score placements. Ported from
/// `sort_model.py`'s `ITEM_FEATURE_NAMES`.
pub const ITEM_FEATURE_NAMES: &[&str] = &[
    "width",
    "height",
    "area",
    "max_side",
    "rarity",
    "slot_x",
    "slot_y",
    "slot_x_norm",
    "slot_y_norm",
    "pack_mode",
    "stack_mode",
    "free_ratio",
    "distance_from_current",
    "blockers_at_target",
    "neighbor_rarity_match",
];

/// Features describing one physical move, reserved for move-failure prediction. Ported from
/// `sort_model.py`'s `MOVE_FEATURE_NAMES`. Python defines this list and records `MOVE_OUTCOME`
/// events for it (see `SortEventStore::record_move_outcome`) but, like Python, never trains a model
/// head from them — no `train_moves` exists on either side.
pub const MOVE_FEATURE_NAMES: &[&str] = &[
    "item_area",
    "move_distance_grid",
    "source_stash_type",
    "dest_stash_type",
    "current_delay",
    "move_index",
    "consecutive_successes",
];

pub const MODEL_SCHEMA_VERSION: u32 = 3;

/// Minimum labeled samples before `train_risk` will fit a new risk model.
pub const MIN_RISK_SAMPLES: usize = 25;
/// Minimum labeled samples before `train_items` will fit a new item model.
pub const MIN_ITEM_SAMPLES: usize = 30;
/// Risk assumed when no model has been trained or loaded yet.
pub const COLD_START_RISK: f64 = 0.15;
/// Extra weight given to samples derived from a user correction.
pub const CORRECTION_WEIGHT: f64 = 3.0;
/// Extra weight given to samples derived from a failure.
pub const FAILURE_WEIGHT: f64 = 2.0;

pub(super) fn sigmoid(x: f64) -> f64 {
    if x > 20.0 {
        1.0
    } else if x < -20.0 {
        0.0
    } else {
        1.0 / (1.0 + (-x).exp())
    }
}

fn linear_score(coefficients: &[f64], intercept: f64, vector: &[f64]) -> f64 {
    coefficients.iter().zip(vector).map(|(c, v)| c * v).sum::<f64>() + intercept
}

/// Builds a feature vector in `names` order, treating a missing key or a non-finite value as 0.0 —
/// the same "never let one bad feature blow up prediction" contract as `sort_model.py`'s
/// `_safe_float`. Unlike Python, callers already hand us typed `f64`s, so there is no type
/// coercion to do, only the presence/finiteness check.
pub(super) fn feature_vector(features: &HashMap<String, f64>, names: &[&str]) -> Vec<f64> {
    names.iter().map(|name| features.get(*name).copied().filter(|v| v.is_finite()).unwrap_or(0.0)).collect()
}

/// A trained linear model, ready to serialize to `risk_model.json` / `item_model.json` or to
/// exchange with `apply_remote_*`. Mirrors the dict `sort_model.py` builds in `_fit`.
#[derive(Debug, Clone, Serialize)]
pub struct ModelPayload {
    pub schema_version: u32,
    pub version: String,
    pub trained_at: f64,
    pub samples: usize,
    pub coefficients: Vec<f64>,
    pub intercept: f64,
    pub feature_names: Vec<String>,
    pub training_score: f64,
    pub model_type: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_at: Option<f64>,
}

/// Checks that a (possibly externally supplied) JSON value has the shape `ModelPayload` needs:
/// `coefficients` is a finite-number array matching `expected_features` in length, `intercept` is
/// a finite number, and `feature_names`, if present at all, matches `expected_features` exactly.
///
/// Ported from `sort_model.py`'s `validate_model_payload`. Operates on [`Value`] rather than a
/// typed struct so a malformed remote payload (wrong types, wrong shape) fails this check and
/// returns `false`, instead of erroring out of a `Deserialize` call — the same tolerance Python's
/// dynamically-typed dict inspection gives it.
pub fn validate_model_payload(payload: &Value, expected_features: &[&str]) -> bool {
    let Some(obj) = payload.as_object() else { return false };

    let Some(coefficients) = obj.get("coefficients").and_then(Value::as_array) else {
        return false;
    };
    if coefficients.len() != expected_features.len() {
        return false;
    }
    if !coefficients.iter().all(|v| v.as_f64().is_some_and(f64::is_finite)) {
        return false;
    }

    let Some(intercept) = obj.get("intercept").and_then(Value::as_f64) else { return false };
    if !intercept.is_finite() {
        return false;
    }

    if let Some(feature_names) = obj.get("feature_names") {
        let Some(names) = feature_names.as_array() else { return false };
        let names: Option<Vec<&str>> = names.iter().map(Value::as_str).collect();
        if names.as_deref() != Some(expected_features) {
            return false;
        }
    }

    true
}

/// Reads a [`ModelPayload`] out of a `Value` already known to pass [`validate_model_payload`] for
/// `expected_features`. Metadata fields absent from a hand-built or minimal payload (as in the
/// ported tests) fall back to neutral defaults, matching Python's use of `dict.get(...)` with no
/// required keys beyond `coefficients` / `intercept`.
fn payload_from_validated_value(value: &Value, expected_features: &[&str]) -> ModelPayload {
    let obj = value.as_object();
    let get_f64 = |key: &str| obj.and_then(|o| o.get(key)).and_then(Value::as_f64);
    let get_str = |key: &str| obj.and_then(|o| o.get(key)).and_then(Value::as_str);

    let coefficients = obj
        .and_then(|o| o.get("coefficients"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();

    ModelPayload {
        schema_version: get_f64("schema_version").unwrap_or(0.0) as u32,
        version: get_str("version").unwrap_or_default().to_string(),
        trained_at: get_f64("trained_at").unwrap_or(0.0),
        samples: get_f64("samples").unwrap_or(0.0) as usize,
        coefficients,
        intercept: get_f64("intercept").unwrap_or(0.0),
        feature_names: expected_features.iter().map(|s| s.to_string()).collect(),
        training_score: get_f64("training_score").unwrap_or(0.0),
        model_type: get_str("model_type").unwrap_or("lr").to_string(),
        source: get_str("source").unwrap_or("local").to_string(),
        received_at: get_f64("received_at"),
    }
}

/// Serializes `value` to `path` via a temp-file-then-rename, so a reader never observes a
/// half-written file — the same atomicity `sort_model.py`'s `_write_json` gets from
/// `tmp.replace(path)`.
fn write_json_atomic(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Reads and parses `path` as JSON, returning `None` on any error (missing file, unreadable,
/// malformed JSON) — mirrors `sort_model.py`'s `_read_json`, which treats any failure as "no cached
/// model" rather than propagating it.
fn read_json_value(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Bookkeeping mirroring `sort_model.py`'s `_meta` dict: when each head last trained and how many
/// samples it has seen in total (across retrains, not just the most recent one).
#[derive(Debug, Clone, Default)]
struct ModelMeta {
    last_risk_train: Option<f64>,
    last_item_train: Option<f64>,
    total_risk_samples_seen: u64,
    total_item_samples_seen: u64,
}

#[derive(Default)]
struct ModelState {
    risk_payload: Option<ModelPayload>,
    item_payload: Option<ModelPayload>,
    meta: ModelMeta,
}

/// Unified model for sort-risk prediction and item-slot scoring.
///
/// Cheap to clone: internally an `Arc`, so handing a clone to a background training thread or to
/// [`super::SortLearningManager`] / [`super::SortObserver`] shares the same underlying state, the
/// same role Python's module-level singleton (`get_sort_adaptive_model()`) plays.
#[derive(Clone)]
pub struct SortAdaptiveModel {
    inner: std::sync::Arc<Inner>,
}

struct Inner {
    risk_model_path: PathBuf,
    item_model_path: PathBuf,
    state: std::sync::Mutex<ModelState>,
    /// One background-training handle per head, so a long risk retrain never blocks scheduling an
    /// item retrain (and vice versa) — see `spawn_head_job`.
    training: std::sync::Mutex<TrainingThreads>,
}

#[derive(Default)]
struct TrainingThreads {
    risk: Option<std::thread::JoinHandle<()>>,
    items: Option<std::thread::JoinHandle<()>>,
}

impl SortAdaptiveModel {
    /// Loads (or initializes) the model rooted at `base_dir`. Matches
    /// `sort_model.py`'s `SortAdaptiveModel(base_dir)`, except `base_dir` is required here: the
    /// Python default of looking up an app-wide output directory belongs to the app that embeds
    /// this crate, not to the model itself.
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        let base_dir = base_dir.as_ref();
        // Best-effort: if this fails, saving later will fail the same way and simply not persist,
        // exactly as Python's `mkdir(parents=True, exist_ok=True)` failing silently would leave it
        // running in-memory-only.
        let _ = fs::create_dir_all(base_dir);

        let risk_model_path = base_dir.join("risk_model.json");
        let item_model_path = base_dir.join("item_model.json");

        let risk_payload = read_json_value(&risk_model_path)
            .filter(|v| validate_model_payload(v, RISK_FEATURE_NAMES))
            .map(|v| payload_from_validated_value(&v, RISK_FEATURE_NAMES));
        let item_payload = read_json_value(&item_model_path)
            .filter(|v| validate_model_payload(v, ITEM_FEATURE_NAMES))
            .map(|v| payload_from_validated_value(&v, ITEM_FEATURE_NAMES));

        SortAdaptiveModel {
            inner: std::sync::Arc::new(Inner {
                risk_model_path,
                item_model_path,
                state: std::sync::Mutex::new(ModelState { risk_payload, item_payload, meta: ModelMeta::default() }),
                training: std::sync::Mutex::new(TrainingThreads::default()),
            }),
        }
    }

    /// When each head last finished training, and how many labeled samples it has seen across all
    /// retrains so far. Exposed for diagnostics/UI; mirrors `sort_model.py`'s `get_meta()`.
    pub fn training_meta(&self) -> (Option<f64>, Option<f64>, u64, u64) {
        let meta = &super::lock(&self.inner.state).meta;
        (meta.last_risk_train, meta.last_item_train, meta.total_risk_samples_seen, meta.total_item_samples_seen)
    }

    /// Probability (0-1) that a sort session with these features will fail. `COLD_START_RISK` when
    /// no risk model has been trained or loaded yet.
    pub fn predict_risk(&self, features: &HashMap<String, f64>) -> f64 {
        let vector = feature_vector(features, RISK_FEATURE_NAMES);
        let state = super::lock(&self.inner.state);
        match &state.risk_payload {
            Some(p) => sigmoid(linear_score(&p.coefficients, p.intercept, &vector)),
            None => COLD_START_RISK,
        }
    }

    /// Score for an (item, slot) candidate, higher is better; raw linear score, not squashed
    /// through a sigmoid, matching Python's `score_item_slot`. `None` when no item model is
    /// available yet (callers should fall back to their own heuristics).
    pub fn score_item_slot(&self, features: &HashMap<String, f64>) -> Option<f64> {
        let vector = feature_vector(features, ITEM_FEATURE_NAMES);
        let state = super::lock(&self.inner.state);
        state.item_payload.as_ref().map(|p| linear_score(&p.coefficients, p.intercept, &vector))
    }

    /// Translates a risk score into a recommended workspace-buffer cell count: `base_min` cells
    /// plus up to `max_extra` more, scaled by risk.
    pub fn recommended_workspace_cells(&self, risk: Option<f64>, base_min: u32, max_extra: u32) -> u32 {
        let r = risk.unwrap_or(COLD_START_RISK).clamp(0.0, 1.0);
        let extra = (f64::from(max_extra) * r).ceil() as u32;
        base_min + extra
    }

    /// The risk model's version string, if one is loaded.
    pub fn risk_version(&self) -> Option<String> {
        super::lock(&self.inner.state).risk_payload.as_ref().map(|p| p.version.clone())
    }

    /// The item model's version string, if one is loaded.
    pub fn item_version(&self) -> Option<String> {
        super::lock(&self.inner.state).item_payload.as_ref().map(|p| p.version.clone())
    }

    /// A copy of the risk model's payload, if one is loaded — for inspection or re-exchange.
    pub fn risk_payload(&self) -> Option<ModelPayload> {
        super::lock(&self.inner.state).risk_payload.clone()
    }

    /// A copy of the item model's payload, if one is loaded — for inspection or re-exchange.
    pub fn item_payload(&self) -> Option<ModelPayload> {
        super::lock(&self.inner.state).item_payload.clone()
    }

    /// Accepts a remote/externally supplied risk model, replacing the current one, when it
    /// validates against [`RISK_FEATURE_NAMES`]. Returns whether it was accepted.
    pub fn apply_remote_risk_model(&self, payload: Value) -> bool {
        self.apply_remote(payload, RISK_FEATURE_NAMES, Head::Risk)
    }

    /// Accepts a remote/externally supplied item model, replacing the current one, when it
    /// validates against [`ITEM_FEATURE_NAMES`]. Returns whether it was accepted.
    pub fn apply_remote_item_model(&self, payload: Value) -> bool {
        self.apply_remote(payload, ITEM_FEATURE_NAMES, Head::Items)
    }

    fn apply_remote(&self, payload: Value, expected_features: &[&str], head: Head) -> bool {
        if !validate_model_payload(&payload, expected_features) {
            return false;
        }
        let mut parsed = payload_from_validated_value(&payload, expected_features);
        if parsed.source.is_empty() {
            parsed.source = "remote".to_string();
        }
        parsed.received_at.get_or_insert_with(super::now_secs);
        self.save_payload(head, parsed);
        true
    }

    /// Updates the in-memory payload first (so the very next prediction reflects it, regardless of
    /// whether the disk write below succeeds) and then best-effort persists it to disk, mirroring
    /// `sort_model.py`'s `_save_risk_payload` / `_save_item_payload` ordering. A failed write here
    /// just means the trained-this-session model will not survive a restart; it does not affect
    /// the model already live in memory, so the error is intentionally not propagated further.
    fn save_payload(&self, head: Head, payload: ModelPayload) {
        let path = match head {
            Head::Risk => self.inner.risk_model_path.clone(),
            Head::Items => self.inner.item_model_path.clone(),
        };
        {
            let mut state = super::lock(&self.inner.state);
            match head {
                Head::Risk => state.risk_payload = Some(payload.clone()),
                Head::Items => state.item_payload = Some(payload.clone()),
            }
        }
        let _ = write_json_atomic(&path, &payload);
    }
}

/// Which head of the model a given operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Head {
    Risk,
    Items,
}

impl ModelPredictor for SortAdaptiveModel {
    fn predict_risk(&self, features: &HashMap<String, f64>) -> f64 {
        SortAdaptiveModel::predict_risk(self, features)
    }

    fn score_item_slot(&self, features: &HashMap<String, f64>) -> Option<f64> {
        SortAdaptiveModel::score_item_slot(self, features)
    }

    fn recommended_workspace_cells(&self, risk: Option<f64>, base_min: u32, max_extra: u32) -> u32 {
        SortAdaptiveModel::recommended_workspace_cells(self, risk, base_min, max_extra)
    }
}

impl Head {
    fn name(self) -> &'static str {
        match self {
            Head::Risk => "risk",
            Head::Items => "item",
        }
    }
}

impl SortAdaptiveModel {
    /// Trains the risk head from labeled session-outcome samples. With `run_async = true`
    /// (Python's default), training happens on a background thread and this returns immediately; a
    /// request for a head that already has a training run in flight is dropped, the same
    /// de-duplication as `sort_model.py`'s `_schedule_train`. Too few samples, or samples of only
    /// one class, silently keep the existing model untouched — there is nothing useful to fit.
    pub fn train_risk(&self, events: Vec<TrainingSample>, run_async: bool) {
        self.train(Head::Risk, events, run_async);
    }

    /// Trains the item head from labeled placement/correction samples. See [`Self::train_risk`]
    /// for the async/dedup contract, which is identical.
    pub fn train_items(&self, events: Vec<TrainingSample>, run_async: bool) {
        self.train(Head::Items, events, run_async);
    }

    fn train(&self, head: Head, events: Vec<TrainingSample>, run_async: bool) {
        if run_async {
            let model = self.clone();
            self.spawn_head_job(head, move || model.fit_and_save(head, events));
        } else {
            self.fit_and_save(head, events);
        }
    }

    /// Runs `job` on a new background thread for `head`, unless one is already running for that
    /// same head, in which case this request is simply dropped. Each head has its own slot, so a
    /// slow risk-training job can never delay scheduling an item-training job or vice versa.
    fn spawn_head_job(&self, head: Head, job: impl FnOnce() + Send + 'static) {
        let mut threads = super::lock(&self.inner.training);
        let slot = match head {
            Head::Risk => &mut threads.risk,
            Head::Items => &mut threads.items,
        };
        if slot.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        *slot = Some(thread::spawn(job));
    }

    fn fit_and_save(&self, head: Head, events: Vec<TrainingSample>) {
        let expected_features = match head {
            Head::Risk => RISK_FEATURE_NAMES,
            Head::Items => ITEM_FEATURE_NAMES,
        };
        let min_samples = match head {
            Head::Risk => MIN_RISK_SAMPLES,
            Head::Items => MIN_ITEM_SAMPLES,
        };

        let Some((vectors, labels, weights)) = prepare_dataset(&events, expected_features, min_samples) else {
            return;
        };

        let (coefficients, intercept) = super::logistic::fit_logistic_regression(&vectors, &labels, &weights);
        let training_score = weighted_accuracy(&vectors, &labels, &weights, &coefficients, intercept);
        let sample_count = labels.len();

        let payload = ModelPayload {
            schema_version: MODEL_SCHEMA_VERSION,
            version: format!("local-{}-{}", head.name(), super::now_secs() as u64),
            trained_at: super::now_secs(),
            samples: sample_count,
            coefficients,
            intercept,
            feature_names: expected_features.iter().map(|s| s.to_string()).collect(),
            training_score,
            model_type: "lr".to_string(),
            source: "local".to_string(),
            received_at: None,
        };

        self.save_payload(head, payload);

        let mut state = super::lock(&self.inner.state);
        match head {
            Head::Risk => {
                state.meta.last_risk_train = Some(super::now_secs());
                state.meta.total_risk_samples_seen += sample_count as u64;
            }
            Head::Items => {
                state.meta.last_item_train = Some(super::now_secs());
                state.meta.total_item_samples_seen += sample_count as u64;
            }
        }
    }
}

/// `(vectors, labels, weights)`, ready to fit a [`super::logistic`] model from.
type Dataset = (Vec<Vec<f64>>, Vec<bool>, Vec<f64>);

/// Extracts a [`Dataset`] from labeled events, flooring each weight the same way
/// `sort_model.py`'s `_prepare_dataset` does (`max(0.1, weight)`). Returns `None` — meaning "keep
/// the existing model, there is nothing useful to learn here" — when there are fewer than
/// `min_samples` events, or when every event shares the same label, since a model cannot separate
/// one class from itself.
fn prepare_dataset(events: &[TrainingSample], feature_names: &[&str], min_samples: usize) -> Option<Dataset> {
    if events.len() < min_samples {
        return None;
    }
    let vectors: Vec<Vec<f64>> = events.iter().map(|e| feature_vector(&e.features, feature_names)).collect();
    let labels: Vec<bool> = events.iter().map(|e| e.label).collect();
    let weights: Vec<f64> =
        events.iter().map(|e| e.weight.max(super::logistic::MIN_SAMPLE_WEIGHT)).collect();

    if !(labels.contains(&true) && labels.contains(&false)) {
        return None;
    }
    Some((vectors, labels, weights))
}

/// Weighted classification accuracy of `(coefficients, intercept)` re-evaluated on its own training
/// set, standing in for `sort_model.py`'s stratified 80/20 holdout score.
///
/// A real holdout split needs randomness to avoid systematically biasing which samples land in
/// which half; this crate deliberately has no RNG dependency (see the module doc comment), and
/// re-scoring on the training set trades a slightly optimistic number for a fully deterministic
/// one. That trade-off is acceptable here because the score is metadata surfaced to the user
/// (`training_score`), not an input to any decision this crate itself makes.
fn weighted_accuracy(
    vectors: &[Vec<f64>],
    labels: &[bool],
    weights: &[f64],
    coefficients: &[f64],
    intercept: f64,
) -> f64 {
    let total_weight: f64 = weights.iter().sum();
    if total_weight <= 0.0 {
        return 0.0;
    }
    let mut correct_weight = 0.0_f64;
    for ((vector, &label), &weight) in vectors.iter().zip(labels).zip(weights) {
        let predicted_positive = linear_score(coefficients, intercept, vector) >= 0.0;
        if predicted_positive == label {
            correct_weight += weight;
        }
    }
    correct_weight / total_weight
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::{Arc, Condvar, Mutex as StdMutex};
    use std::time::Duration;

    fn temp_model() -> (tempfile::TempDir, SortAdaptiveModel) {
        let dir = tempfile::tempdir().expect("create temp dir for test model");
        let model = SortAdaptiveModel::new(dir.path());
        (dir, model)
    }

    /// Blocks until someone flips `gate.0` to `true` and notifies `gate.1`, or 3 seconds pass.
    /// Stands in for the Python test's single shared `threading.Event().wait(3)`.
    fn block_until_released(gate: &(StdMutex<bool>, Condvar)) {
        let (lock, cvar) = gate;
        let mut released = lock.lock().unwrap_or_else(|e| e.into_inner());
        while !*released {
            let (guard, timeout) =
                cvar.wait_timeout(released, Duration::from_secs(3)).unwrap_or_else(|e| e.into_inner());
            released = guard;
            if timeout.timed_out() {
                break;
            }
        }
    }

    fn release(gate: &Arc<(StdMutex<bool>, Condvar)>) {
        let (lock, cvar) = &**gate;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cvar.notify_all();
    }

    // Port of the threading half of `test_sort_model_remote_apply_replaces_live_estimator_and_
    // heads_train_independently` (sort_model.py tests). Python swaps out `_do_train_risk` /
    // `_do_train_items` with blocking test doubles; this port swaps the job closure passed to the
    // private `spawn_head_job` instead, since Rust has no method-level monkeypatching. The other
    // half of that Python test — remote-apply and reload-invalid-cache behaviour, which only needs
    // public API — lives in `tests/learn_model.rs`.
    #[test]
    fn scheduling_one_head_does_not_block_scheduling_the_other() {
        // Arrange
        let (_dir, model) = temp_model();
        let gate = Arc::new((StdMutex::new(false), Condvar::new()));
        let (risk_started_tx, risk_started_rx) = mpsc::channel();
        let (items_started_tx, items_started_rx) = mpsc::channel();

        // Act: start "risk" training, which blocks until released.
        let risk_gate = Arc::clone(&gate);
        model.spawn_head_job(Head::Risk, move || {
            let _ = risk_started_tx.send(());
            block_until_released(&risk_gate);
        });
        risk_started_rx.recv_timeout(Duration::from_secs(2)).expect("risk training should start promptly");

        // Scheduling "items" while "risk" is still in flight must not be dropped or delayed.
        let items_gate = Arc::clone(&gate);
        model.spawn_head_job(Head::Items, move || {
            let _ = items_started_tx.send(());
            block_until_released(&items_gate);
        });

        // Assert
        assert!(
            items_started_rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "item training was dropped while risk training was active"
        );

        release(&gate);
    }

    #[test]
    fn rescheduling_the_same_head_while_running_is_a_no_op() {
        // Arrange
        let (_dir, model) = temp_model();
        let gate = Arc::new((StdMutex::new(false), Condvar::new()));
        let (started_tx, started_rx) = mpsc::channel::<()>();

        // Act: start a "risk" job that blocks, then immediately request another "risk" job.
        let first_gate = Arc::clone(&gate);
        let first_tx = started_tx.clone();
        model.spawn_head_job(Head::Risk, move || {
            let _ = first_tx.send(());
            block_until_released(&first_gate);
        });
        started_rx.recv_timeout(Duration::from_secs(2)).expect("first job should start");

        model.spawn_head_job(Head::Risk, move || {
            let _ = started_tx.send(());
        });

        // Assert: the second request never runs while the first head is still occupied.
        assert!(
            started_rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a second job for the same head should have been dropped, not scheduled"
        );

        release(&gate);
    }

    #[test]
    fn feature_vector_defaults_missing_and_non_finite_values_to_zero() {
        // Arrange
        let mut features = HashMap::new();
        features.insert("a".to_string(), 2.0);
        features.insert("b".to_string(), f64::NAN);

        // Act
        let vector = feature_vector(&features, &["a", "b", "c"]);

        // Assert
        assert_eq!(vector, vec![2.0, 0.0, 0.0]);
    }

    #[test]
    fn prepare_dataset_rejects_too_few_samples() {
        // Arrange
        let events = vec![TrainingSample { features: HashMap::new(), label: true, weight: 1.0 }];

        // Act / Assert
        assert!(prepare_dataset(&events, RISK_FEATURE_NAMES, MIN_RISK_SAMPLES).is_none());
    }

    #[test]
    fn prepare_dataset_rejects_a_single_class() {
        // Arrange: enough samples, but every one of them is a failure.
        let events: Vec<TrainingSample> = (0..MIN_RISK_SAMPLES)
            .map(|_| TrainingSample { features: HashMap::new(), label: true, weight: 1.0 })
            .collect();

        // Act / Assert
        assert!(prepare_dataset(&events, RISK_FEATURE_NAMES, MIN_RISK_SAMPLES).is_none());
    }

    #[test]
    fn prepare_dataset_floors_non_positive_sample_weight() {
        // Arrange
        let mut events: Vec<TrainingSample> = (0..MIN_RISK_SAMPLES)
            .map(|_| TrainingSample { features: HashMap::new(), label: true, weight: 1.0 })
            .collect();
        events.extend(
            (0..MIN_RISK_SAMPLES)
                .map(|_| TrainingSample { features: HashMap::new(), label: false, weight: -5.0 }),
        );

        // Act
        let (_, _, weights) =
            prepare_dataset(&events, RISK_FEATURE_NAMES, MIN_RISK_SAMPLES).expect("dataset should be accepted");

        // Assert
        assert!(weights.iter().all(|&w| w > 0.0), "a negative weight must be floored, not passed through");
    }

    #[test]
    fn cold_start_risk_is_returned_before_any_model_is_trained_or_loaded() {
        // Arrange
        let (_dir, model) = temp_model();

        // Act / Assert
        assert_eq!(model.predict_risk(&HashMap::new()), COLD_START_RISK);
        assert_eq!(model.score_item_slot(&HashMap::new()), None);
    }
}
