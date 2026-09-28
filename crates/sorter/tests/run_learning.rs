//! Feeding a run into sort learning: session features, plan metrics, and one outcome per move.

mod run_fakes;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use input::Cancel;
use run_fakes::*;
use sorter::learn::{EventSink, ModelPredictor, SortObserver, StoreError, UserCorrection, MOVE_FEATURE_NAMES};
use sorter::run::{plan_metrics, session_features, LearningRecorder, MoveOutcomeSink, RunSteps};
use state::stash::STORAGE;

struct Model;

impl ModelPredictor for Model {
    fn predict_risk(&self, _features: &HashMap<String, f64>) -> f64 {
        0.2
    }
    fn score_item_slot(&self, _features: &HashMap<String, f64>) -> Option<f64> {
        None
    }
    fn recommended_workspace_cells(&self, _risk: Option<f64>, base_min: u32, _max_extra: u32) -> u32 {
        base_min
    }
}

/// Keeps the metadata of every finished session.
#[derive(Default)]
struct Events(Mutex<Vec<serde_json::Value>>);

impl EventSink for Events {
    fn record_sort_started(&self, _: &str, _: &HashMap<String, f64>, _: serde_json::Value) -> Result<i64, StoreError> {
        Ok(1)
    }
    fn record_sort_completed(&self, _: &str, _: &HashMap<String, f64>, _: bool, metadata: serde_json::Value) -> Result<i64, StoreError> {
        self.0.lock().unwrap().push(metadata);
        Ok(2)
    }
    fn record_item_placement(&self, _: &str, _: &HashMap<String, f64>, _: bool, _: serde_json::Value) -> Result<i64, StoreError> {
        Ok(3)
    }
    fn record_user_correction(&self, _: &str, _: UserCorrection) -> Result<Vec<i64>, StoreError> {
        Ok(Vec::new())
    }
    fn apply_user_feedback(&self, _: &str, _: bool, _: Option<&str>) -> Result<bool, StoreError> {
        Ok(true)
    }
}

type MoveRecord = (String, HashMap<String, f64>, bool, u32, String);

#[derive(Default)]
struct Moves(Mutex<Vec<MoveRecord>>);

impl MoveOutcomeSink for Moves {
    fn record_move(&self, session_id: &str, features: &HashMap<String, f64>, verified: bool, attempts: u32, item_id: &str, _reason: Option<&str>) {
        self.0.lock().unwrap().push((session_id.into(), features.clone(), verified, attempts, item_id.into()));
    }
}

#[test]
fn session_features_count_the_stash_and_the_bag() {
    let character = scattered();
    let steps = RunSteps::new(&character, &catalog(), STORAGE, &plan_for(&character, false, Vec::new())).expect("plan fits");

    let features = session_features(steps.initial());

    // Two 2x2 shields, two 1x2 daggers, three 1x1 items; one ring in the 10x5 bag.
    let expected = HashMap::from([
        ("stash_total_cells".to_string(), 240.0),
        ("stash_occupied_cells".to_string(), 15.0),
        ("inventory_total_cells".to_string(), 50.0),
        ("inventory_free_cells".to_string(), 49.0),
        ("largest_item_area".to_string(), 4.0),
    ]);
    assert_eq!(features, expected);
}

#[test]
fn plan_metrics_use_the_names_the_risk_model_reads() {
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let steps = RunSteps::new(&character, &catalog(), STORAGE, &plan).expect("plan fits");

    let metrics: HashMap<&str, f64> = plan_metrics(&steps, &plan).into_iter().collect();

    assert_eq!(metrics["plan_size"], 7.0);
    assert_eq!(metrics["largest_item_area"], 4.0);
    assert_eq!(metrics["planned_moves"], plan.moves.len() as f64);
    assert_eq!(metrics["plan_items_needing_move"] + metrics["plan_items_already_placed"], 7.0);
    assert!(metrics.contains_key("park_attempts") && metrics.contains_key("buffered_items"));
}

#[test]
fn every_move_is_recorded_and_counted_on_the_session() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let steps = RunSteps::new(&character, &catalog(), STORAGE, &plan).expect("plan fits");
    let events = Arc::new(Events::default());
    let observer = SortObserver::without_immediate_retrain(Arc::new(Model), events.clone());
    let mut handle = observer.begin_session(Some(character.id.clone()), Some(STORAGE), false, false, session_features(steps.initial()));
    let moves = Moves::default();
    let game = FakeGame::new(&character, &catalog(), STORAGE);

    // Act
    let report = {
        let mut inner = ();
        let mut recorder = LearningRecorder::new(&mut handle, Some(&moves), &mut inner);
        run(&game, &character, &plan, &config(None), &Cancel::new(), &mut recorder)
    };
    let session_id = handle.session_id().to_string();
    handle.finalize(report.is_complete(), false, None);

    // Assert
    assert!(report.is_complete(), "{report:?}");
    let recorded = moves.0.lock().unwrap();
    assert_eq!(recorded.len(), report.done);
    for (session, features, verified, attempts, _) in recorded.iter() {
        assert_eq!(session, &session_id);
        assert!(*verified && *attempts == 1);
        for name in MOVE_FEATURE_NAMES {
            assert!(features.contains_key(*name), "missing move feature {name}");
        }
    }
    let completed = events.0.lock().unwrap();
    assert_eq!(completed[0]["moves_executed"], serde_json::json!(report.done as f64));
    assert_eq!(completed[0]["moves_verified"], serde_json::json!(report.done as f64));
}
