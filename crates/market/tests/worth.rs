//! The Rust worth model against answers from the Python model it was ported from
//! (tests/fixtures/worth_golden.json, made by predicting a small hand-made model in Python).

use market::{Confidence, WorthModel};
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("fixtures/worth_golden.json")).unwrap()
}

fn close(actual: f64, expected: f64, what: &str) {
    let tolerance = 1e-9 * expected.abs().max(1.0);
    assert!((actual - expected).abs() <= tolerance, "{what}: {actual} != {expected}");
}

#[test]
fn predictions_match_the_python_model() {
    let golden = golden();
    let model = WorthModel::from_json(&golden["model"].to_string()).unwrap();

    for case in golden["cases"].as_array().unwrap() {
        let item = case["item_id"].as_str().unwrap();
        let rolls: Vec<(String, f64)> = case["rolls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r[0].as_str().unwrap().to_owned(), r[1].as_f64().unwrap()))
            .collect();
        let quantity = case["quantity"].as_u64().unwrap() as u32;
        let slot = case["slot"].as_str();
        let rarity = case["rarity"].as_i64();
        let expected = &case["expected"];

        let estimate = model.predict(item, &rolls, quantity, slot, rarity);

        for (field, actual) in [
            ("value", estimate.value),
            ("floor", estimate.floor),
            ("low", estimate.low),
            ("high", estimate.high),
            ("typical", estimate.typical),
        ] {
            close(actual, expected[field].as_f64().unwrap(), &format!("{item} {field}"));
        }
        let confidence = match estimate.confidence {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
            Confidence::Unknown => "unknown",
        };
        assert_eq!(confidence, expected["confidence"].as_str().unwrap(), "{item} confidence");
        assert_eq!(estimate.listings, expected["listings"].as_u64().unwrap(), "{item} listings");

        let expected_rolls = expected["rolls"].as_array().unwrap();
        assert_eq!(estimate.rolls.len(), expected_rolls.len(), "{item} rolls");
        for (roll, want) in estimate.rolls.iter().zip(expected_rolls) {
            assert_eq!(roll.stat, want[0].as_str().unwrap());
            assert!((roll.quality - want[1].as_f64().unwrap()).abs() < 0.0006, "{item} {} quality", roll.stat);
            assert!((roll.effect_pct - want[2].as_f64().unwrap()).abs() < 0.051, "{item} {} effect", roll.stat);
        }

        let expected_pairs: Vec<(String, f64)> = expected["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_str().unwrap().to_owned(), p[1].as_f64().unwrap()))
            .collect();
        let pairs: Vec<(String, f64)> = estimate.pairs.iter().map(|p| (p.stats.join("+"), p.effect_pct)).collect();
        assert_eq!(pairs.len(), expected_pairs.len(), "{item} pairs {pairs:?}");
        for ((stats, pct), (want_stats, want_pct)) in pairs.iter().zip(&expected_pairs) {
            assert_eq!(stats, want_stats);
            assert!((pct - want_pct).abs() < 0.051, "{item} {stats} pair effect");
        }
    }
}

#[test]
fn a_saved_model_of_another_version_is_refused() {
    assert!(WorthModel::from_json(r#"{"version": 2, "intercept": 1.0}"#).is_err());
    assert!(WorthModel::from_json("not json").is_err());
}

#[test]
fn what_the_model_knows() {
    let model = WorthModel::from_json(&golden()["model"].to_string()).unwrap();
    assert!(model.knows("Robe_5001"));
    assert!(!model.knows("Mystery_5001"));
    assert_eq!(model.listings(), 58);
}
