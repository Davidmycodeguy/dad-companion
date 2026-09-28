//! Port of `test_sort_model_remote_apply_replaces_live_estimator_and_heads_train_independently`
//! (DnDTools `UI/tests/test_stash_search.py`) — the public-API portion: validating and applying a
//! remote model payload, and treating a corrupted cached model file as "no model".
//!
//! The Python test also swaps out `SortAdaptiveModel._do_train_risk` / `_do_train_items` with
//! blocking test doubles to prove that training one head never blocks scheduling the other. Rust
//! has no method-level monkeypatching, so that part is ported instead as a white-box test next to
//! the private scheduling code it exercises: see
//! `scheduling_one_head_does_not_block_scheduling_the_other` and
//! `rescheduling_the_same_head_while_running_is_a_no_op` in `crates/sorter/src/learn/model.rs`.

use std::collections::HashMap;

use sorter::learn::{SortAdaptiveModel, ITEM_FEATURE_NAMES, RISK_FEATURE_NAMES};

fn temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create temp dir for test model")
}

#[test]
fn a_valid_remote_risk_model_replaces_the_live_one_and_predicts_immediately() {
    // Arrange
    let dir = temp_dir();
    let model = SortAdaptiveModel::new(dir.path());
    let payload = serde_json::json!({
        "version": "remote-risk",
        "coefficients": vec![0.0; RISK_FEATURE_NAMES.len()],
        "intercept": -2.0,
        "feature_names": RISK_FEATURE_NAMES,
    });

    // Act
    let accepted = model.apply_remote_risk_model(payload);

    // Assert: zero coefficients + intercept -2.0 => sigmoid(-2.0) ~= 0.12, well under 0.2.
    assert!(accepted);
    assert!(model.predict_risk(&HashMap::new()) < 0.2);
}

#[test]
fn a_remote_risk_model_with_non_numeric_coefficients_is_rejected() {
    // Arrange
    let dir = temp_dir();
    let model = SortAdaptiveModel::new(dir.path());
    let payload = serde_json::json!({
        "version": "remote-risk",
        "coefficients": vec!["bad"; RISK_FEATURE_NAMES.len()],
        "intercept": -2.0,
        "feature_names": RISK_FEATURE_NAMES,
    });

    // Act / Assert
    assert!(!model.apply_remote_risk_model(payload));
}

#[test]
fn a_remote_risk_model_whose_feature_names_do_not_match_is_rejected() {
    // Arrange: `feature_names` present but empty, so it cannot match `RISK_FEATURE_NAMES`.
    let dir = temp_dir();
    let model = SortAdaptiveModel::new(dir.path());
    let payload = serde_json::json!({
        "version": "remote-risk",
        "coefficients": vec![0.0; RISK_FEATURE_NAMES.len()],
        "intercept": -2.0,
        "feature_names": [],
    });

    // Act / Assert
    assert!(!model.apply_remote_risk_model(payload));
}

#[test]
fn a_remote_risk_model_whose_feature_names_are_the_wrong_json_type_is_rejected() {
    // Arrange: `feature_names` present but not even an array.
    let dir = temp_dir();
    let model = SortAdaptiveModel::new(dir.path());
    let payload = serde_json::json!({
        "version": "remote-risk",
        "coefficients": vec![0.0; RISK_FEATURE_NAMES.len()],
        "intercept": -2.0,
        "feature_names": 5,
    });

    // Act / Assert
    assert!(!model.apply_remote_risk_model(payload));
}

#[test]
fn a_valid_remote_item_model_scores_with_an_unsquashed_linear_score() {
    // Arrange
    let dir = temp_dir();
    let model = SortAdaptiveModel::new(dir.path());
    let payload = serde_json::json!({
        "version": "remote-item",
        "coefficients": vec![0.0; ITEM_FEATURE_NAMES.len()],
        "intercept": 7.0,
        "feature_names": ITEM_FEATURE_NAMES,
    });

    // Act
    let accepted = model.apply_remote_item_model(payload);

    // Assert: zero coefficients + intercept 7.0 => raw score of 7.0, not sigmoid(7.0).
    assert!(accepted);
    assert_eq!(model.score_item_slot(&HashMap::new()), Some(7.0));
}

#[test]
fn a_corrupted_cached_risk_model_file_loads_as_no_model_at_all() {
    // Arrange: hand-write a risk_model.json whose coefficients are not numbers, exactly the shape
    // `validate_model_payload` must reject on load, not just on `apply_remote_risk_model`.
    let dir = temp_dir();
    let bad_payload = serde_json::json!({
        "version": "invalid-cached-version",
        "coefficients": vec!["bad"; RISK_FEATURE_NAMES.len()],
        "intercept": 0.0,
        "feature_names": RISK_FEATURE_NAMES,
    });
    std::fs::write(dir.path().join("risk_model.json"), bad_payload.to_string()).expect("write test fixture");

    // Act
    let model = SortAdaptiveModel::new(dir.path());

    // Assert
    assert_eq!(model.risk_version(), None);
}
