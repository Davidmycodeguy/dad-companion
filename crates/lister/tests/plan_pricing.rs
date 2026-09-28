//! Port of the `apply_game_prices` / `price_from_model` half of DnDTools'
//! `tests/test_market_lister_plan.py` (the `build_plan` / `PlanEntry` half is `tests/plan.rs`).

mod common;

use std::collections::{HashMap, HashSet};

use common::item;
use lister::plan::{apply_game_prices, build_plan, price_from_model, MarketBucket, ModelPrice, PlanEntry, MAX_LISTING_PRICE};
use market::{Confidence, Estimate, ListerRules, MarketRow};
use serde_json::{json, Value};

fn row(item_id: &str, price: i64) -> MarketRow {
    row_full(item_id, price, vec![], "")
}

fn row_id(item_id: &str, price: i64, listing_id: &str) -> MarketRow {
    row_full(item_id, price, vec![], listing_id)
}

fn row_full(item_id: &str, price: i64, rolls: Vec<(String, i64)>, listing_id: &str) -> MarketRow {
    MarketRow { item_id: item_id.to_string(), price, base: vec![], rolls, listing_id: listing_id.to_string(), count: 1 }
}

fn bucket(all: Vec<MarketRow>) -> MarketBucket {
    MarketBucket { same: vec![], all, ..Default::default() }
}

fn estimate(value: f64, floor: f64) -> Estimate {
    estimate_with(value, floor, Confidence::Unknown)
}

fn estimate_with(value: f64, floor: f64, confidence: Confidence) -> Estimate {
    Estimate { value, floor, low: 0.0, high: 0.0, confidence, typical: 0.0, rolls: vec![], pairs: vec![], listings: 0 }
}

fn rules(source_stash_ids: &[&str]) -> ListerRules {
    ListerRules { source_stash_ids: source_stash_ids.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

/// The unpriced entries `build_plan` produces without a `price_lookup` — the starting point every
/// `apply_game_prices` test works from, matching the Python `_plan(stashes, None).entries` helper.
fn unpriced_entries(stashes: &HashMap<String, Vec<Value>>, rules: &ListerRules) -> Vec<PlanEntry> {
    build_plan(stashes, rules, None, &[4, 20, 5, 6, 7, 8, 9, 30], Some(38), Some(10.0), &|| {}, &HashSet::new()).unwrap().entries
}

#[test]
fn apply_game_prices_undercuts_cheapest_listing() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({})), item("b", 1, json!({})), item("c", 2, json!({}))])]);
    let unpriced = unpriced_entries(&stashes, &rules(&["2"]));
    let market = HashMap::from([
        ("a".to_string(), bucket(vec![row("Id_a", 300), row("Id_a", 310), row("Id_a", 333), row("Id_a", 350)])),
        ("b".to_string(), bucket(vec![])),
        ("c".to_string(), bucket(vec![row("Id_other", 50), row("Id_c", 1000), row("Id_c", 1000), row("Id_c", 1100)])),
    ]);
    let plan = apply_game_prices(&unpriced, &market, &rules(&["2"]), None, 0.25, &HashSet::new(), None, None, None);
    assert_eq!(
        plan.entries.iter().map(|e| (e.unique_id.as_str(), e.price, e.fee)).collect::<Vec<_>>(),
        [("a", 270, 15), ("c", 900, 45)]
    );
    assert_eq!(
        plan.skipped.iter().map(|s| (s.name.as_str(), s.reason.as_str())).collect::<Vec<_>>(),
        [("Item b", "nobody is selling this right now")]
    );
}

#[test]
fn apply_game_prices_marks_unsearched_items_not_priced() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({}))])]);
    let unpriced = unpriced_entries(&stashes, &rules(&["2"]));
    let plan = apply_game_prices(&unpriced, &HashMap::new(), &rules(&["2"]), None, 0.25, &HashSet::new(), None, None, None);
    assert_eq!(
        plan.skipped.iter().map(|s| (s.name.as_str(), s.reason.as_str())).collect::<Vec<_>>(),
        [("Item a", "not priced — the pricing run stopped first")]
    );
}

/// Regression test: `extra_rows` must accept a closure borrowing ordinary local data (here, a
/// local `history` map) — a bare, lifetime-free `type ExtraRows = dyn Fn(...);` alias would
/// silently require `'static` and reject exactly this, the closure's real-world shape.
#[test]
fn apply_game_prices_merges_history_rows_and_records_confidence() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({}))])]);
    let unpriced = unpriced_entries(&stashes, &rules(&["2"]));
    let history = HashMap::from([("Id_a".to_string(), vec![row("Id_a", 300), row("Id_a", 320), row("Id_a", 340)])]);
    let extra_rows = |item_id: &str| history.get(item_id).cloned().unwrap_or_default();
    let market = HashMap::from([("a".to_string(), bucket(vec![]))]);
    let plan = apply_game_prices(&unpriced, &market, &rules(&["2"]), Some(&extra_rows), 0.25, &HashSet::new(), None, None, None);
    assert_eq!(
        plan.entries.iter().map(|e| (e.unique_id.as_str(), e.price, e.confidence.as_str())).collect::<Vec<_>>(),
        [("a", 270, "high")]
    );
}

#[test]
fn apply_game_prices_excludes_our_own_listings_and_records_recommended() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({}))])]);
    let unpriced = unpriced_entries(&stashes, &rules(&["2"]));
    let market =
        HashMap::from([("a".to_string(), bucket(vec![row_id("Id_a", 90, "mine"), row_id("Id_a", 300, "x1"), row_id("Id_a", 310, "x2")]))]);
    let exclude = HashSet::from(["mine".to_string()]);
    let plan = apply_game_prices(&unpriced, &market, &rules(&["2"]), None, 0.25, &exclude, None, None, None);
    let entry = &plan.entries[0];
    assert_eq!((entry.price, entry.recommended), (270, 270));
}

#[test]
fn apply_game_prices_skips_prices_above_the_game_maximum() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({}))])]);
    let unpriced = unpriced_entries(&stashes, &rules(&["2"]));
    let market = HashMap::from([("a".to_string(), bucket(vec![row("Id_a", MAX_LISTING_PRICE * 3)]))]);
    let plan = apply_game_prices(&unpriced, &market, &rules(&["2"]), None, 0.25, &HashSet::new(), None, None, None);
    assert!(plan.entries.is_empty());
    assert!(plan.skipped[0].reason.contains("maximum"));
}

#[test]
fn game_prices_are_capped_by_the_value_model() {
    let entry = PlanEntry { rolls: vec![("Luck".to_string(), 17)], ..PlanEntry::new("a", "Helm", 5, "2", 0, 1, 1, 0, 0, 10) };
    let entry = PlanEntry { item_id: "Helm_5001".to_string(), ..entry };
    let market = HashMap::from([(
        "a".to_string(),
        bucket(vec![
            row_full("Helm_5001", 700, vec![("Luck".to_string(), 17), ("Vigor".to_string(), 3)], "1"),
            row_full("Helm_5001", 300, vec![("Strength".to_string(), 1)], "2"),
        ]),
    )]);
    let priced_rules = ListerRules { min_price: 50, ..Default::default() };
    let entries = [entry.clone()];
    let plain = apply_game_prices(&entries, &market, &priced_rules, None, 0.25, &HashSet::new(), None, None, None);
    assert_eq!(plain.entries[0].price, 630);

    // Regression: `worth` must accept a closure borrowing local data, not just a `'static` one.
    let model_value = 400.0;
    let worth = |_e: &PlanEntry| Some(ModelPrice::Value(model_value));
    let capped = apply_game_prices(&entries, &market, &priced_rules, None, 0.25, &HashSet::new(), None, Some(&worth), None);
    assert_eq!(capped.entries[0].price, 360);
    assert!(capped.entries[0].compared.contains("value model"));
}

#[test]
fn game_prices_use_the_value_models_lowest_reasonable_price() {
    let entry = PlanEntry { rolls: vec![("Luck".to_string(), 17)], item_id: "Helm_5001".to_string(), ..PlanEntry::new("a", "Helm", 5, "2", 0, 1, 1, 0, 0, 10) };
    let market = HashMap::from([(
        "a".to_string(),
        bucket(vec![row_full("Helm_5001", 700, vec![("Luck".to_string(), 17)], "1"), row_full("Helm_5001", 300, vec![("Strength".to_string(), 1)], "2")]),
    )]);
    let rules = ListerRules { min_price: 50, ..Default::default() };
    let model = estimate(800.0, 560.0);
    let worth = |_e: &PlanEntry| Some(ModelPrice::Estimate(model.clone()));
    let plan = apply_game_prices(&[entry], &market, &rules, None, 0.25, &HashSet::new(), None, Some(&worth), None);
    assert_eq!(plan.entries[0].price, 560);
}

#[test]
fn formula_pricing_lists_at_the_lowest_reasonable_price() {
    let helm = PlanEntry { rolls: vec![("Luck".to_string(), 17)], item_id: "Helm_5001".to_string(), ..PlanEntry::new("a", "Helm", 5, "2", 0, 1, 1, 0, 0, 10) };
    let cheap = PlanEntry { item_id: "Cap_5001".to_string(), ..PlanEntry::new("b", "Cap", 5, "2", 1, 1, 1, 0, 0, 10) };
    let rich_vendor = PlanEntry { item_id: "Crown_5001".to_string(), ..PlanEntry::new("c", "Crown", 5, "2", 2, 1, 1, 0, 0, 900) };
    let unknown = PlanEntry { item_id: "Rock_1001".to_string(), ..PlanEntry::new("d", "Rock", 5, "2", 3, 1, 1, 0, 0, 1) };
    let estimates = HashMap::from([
        ("a".to_string(), estimate_with(400.0, 325.7, Confidence::High)),
        ("b".to_string(), estimate_with(40.0, 30.0, Confidence::High)),
        ("c".to_string(), estimate_with(900.0, 800.0, Confidence::Medium)),
    ]);
    let worth = |e: &PlanEntry| estimates.get(&e.unique_id).cloned();
    let rules = ListerRules { min_price: 50, ..Default::default() };
    let plan = price_from_model(&[helm, cheap, rich_vendor, unknown], &rules, Some(&worth));
    assert_eq!(plan.entries.iter().map(|e| (e.unique_id.as_str(), e.price, e.recommended)).collect::<Vec<_>>(), [("a", 325, 325)]);
    assert!(plan.entries[0].compared.contains("lowest reasonable"));
    let skip_reasons: HashMap<&str, &str> = plan.skipped.iter().map(|s| (s.name.as_str(), s.reason.as_str())).collect();
    assert_eq!(
        skip_reasons,
        HashMap::from([("Cap", "below min price"), ("Crown", "vendor pays more"), ("Rock", "no market data for the value formula")])
    );
}

#[test]
fn every_skip_names_its_item_so_it_can_be_sold_to_a_merchant() {
    let stashes = HashMap::from([(
        "2".to_string(),
        vec![
            item("gold", 0, json!({"itemId": "GoldCoins"})),
            item("low", 1, json!({"rarity": 1})),
            item("a", 2, json!({})),
            item("b", 3, json!({})),
        ],
    )]);
    let plan = build_plan(&stashes, &rules(&["2"]), None, &[4, 20, 5, 6, 7, 8, 9, 30], Some(38), Some(10.0), &|| {}, &HashSet::new()).unwrap();
    let names: HashMap<&str, &str> = plan.skipped.iter().map(|s| (s.name.as_str(), s.unique_id.as_str())).collect();
    assert_eq!(names, HashMap::from([("Item gold", "gold"), ("Item low", "low")]));

    let unpriced = plan.entries;
    let market = HashMap::from([("a".to_string(), bucket(vec![row("Id_a", 20), row("Id_a", 20), row("Id_a", 20)]))]);
    let priced = apply_game_prices(&unpriced, &market, &rules(&["2"]), None, 0.25, &HashSet::new(), None, None, None);
    let names: HashMap<&str, &str> = priced.skipped.iter().map(|s| (s.name.as_str(), s.unique_id.as_str())).collect();
    assert_eq!(names, HashMap::from([("Item a", "a"), ("Item b", "b")]));

    let no_worth = |_e: &PlanEntry| None;
    let modelled = price_from_model(&unpriced, &ListerRules { min_price: 50, ..Default::default() }, Some(&no_worth));
    let ids: HashSet<&str> = modelled.skipped.iter().map(|s| s.unique_id.as_str()).collect();
    assert_eq!(ids, HashSet::from(["a", "b"]));
}

#[test]
fn plan_json_flags_skips_a_merchant_should_buy() {
    let stashes =
        HashMap::from([("2".to_string(), vec![item("gold", 0, json!({"itemId": "GoldCoins"})), item("low", 1, json!({"rarity": 1}))])]);
    let plan = build_plan(&stashes, &rules(&["2"]), None, &[4, 20, 5, 6, 7, 8, 9, 30], Some(38), Some(10.0), &|| {}, &HashSet::new()).unwrap();
    let dict = plan.to_dict();
    let flags: HashMap<String, bool> =
        dict["skipped"].as_array().unwrap().iter().map(|s| (s["unique_id"].as_str().unwrap().to_string(), s["merchant"].as_bool().unwrap())).collect();
    assert_eq!(flags, HashMap::from([("gold".to_string(), false), ("low".to_string(), true)]));
}
