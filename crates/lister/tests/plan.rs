//! Port of DnDTools' `tests/test_market_lister_plan.py` (the `build_plan` / `PlanEntry` half; the
//! `apply_game_prices` / `price_from_model` half is `tests/plan_pricing.rs`).

mod common;

use std::collections::{HashMap, HashSet};

use common::item;
use lister::plan::{build_plan, PlanEntry, PlanEntryError, MAX_LISTING_PRICE};
use market::ListerRules;
use serde_json::{json, Value};

fn ok(price: i64) -> Value {
    json!({"success": true, "has_data": true, "avg_price": price, "lowest_ask": price, "num_listings": 10})
}

fn tab_mapping() -> Vec<i64> {
    vec![4, 20, 5, 6, 7, 8, 9, 30]
}

/// Mirrors the Python `_plan(stashes, lookup, rules=None, **overrides)` test helper: everything not
/// explicitly overridden takes the same defaults the Python tests rely on.
struct PlanCall {
    tab_mapping: Vec<i64>,
    free_spots: Option<i64>,
    data_age_s: Option<f64>,
    exclude_unique_ids: HashSet<String>,
}

impl Default for PlanCall {
    fn default() -> Self {
        PlanCall { tab_mapping: tab_mapping(), free_spots: Some(38), data_age_s: Some(10.0), exclude_unique_ids: HashSet::new() }
    }
}

fn plan(
    stashes: &HashMap<String, Vec<Value>>,
    lookup: Option<&dyn Fn(&Value) -> Value>,
    rules: &ListerRules,
    call: PlanCall,
) -> Result<lister::plan::Plan, lister::plan::PlanError> {
    build_plan(stashes, rules, lookup, &call.tab_mapping, call.free_spots, call.data_age_s, &|| {}, &call.exclude_unique_ids)
}

fn rules(source_stash_ids: &[&str]) -> ListerRules {
    ListerRules { source_stash_ids: source_stash_ids.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

#[test]
fn build_plan_prices_and_orders_entries() {
    let stashes = HashMap::from([
        ("2".to_string(), vec![item("a", 3, json!({}))]),
        ("4".to_string(), vec![item("b", 0, json!({"width": 2, "height": 3}))]),
    ]);
    let lookup = |_item: &Value| ok(1000);
    let result = plan(&stashes, Some(&lookup), &rules(&["2", "4"]), PlanCall::default()).unwrap();
    assert_eq!(result.entries.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    let b = &result.entries[1];
    assert_eq!((b.stash_id.as_str(), b.slot_id, b.width, b.height, b.price, b.fee), ("4", 0, 2, 3, 900, 45));
    assert!(result.warnings.is_empty());
}

#[test]
fn build_plan_records_price_skips() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({})), item("b", 1, json!({}))])]);
    let lookup = |item: &Value| if item["itemUniqueId"] == "a" { ok(1000) } else { json!({"success": true, "has_data": false}) };
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), PlanCall::default()).unwrap();
    assert_eq!(result.entries.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(
        result.skipped.iter().map(|s| (s.name.as_str(), s.reason.as_str())).collect::<Vec<_>>(),
        [("Item b", "no market data")]
    );
}

#[test]
fn build_plan_caps_to_free_spots_and_max_items() {
    let stashes = HashMap::from([("2".to_string(), (0..6).map(|i| item(&i.to_string(), i, json!({}))).collect())]);
    let lookup = |_item: &Value| ok(1000);
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), PlanCall { free_spots: Some(2), ..Default::default() }).unwrap();
    assert_eq!(result.entries.len(), 2);
    assert!(result.warnings.iter().any(|w| w.contains("free listing spots")));

    let capped_rules = ListerRules { max_items_per_run: 3, ..rules(&["2"]) };
    let uncapped = plan(&stashes, Some(&lookup), &capped_rules, PlanCall { free_spots: None, ..Default::default() }).unwrap();
    assert_eq!(uncapped.entries.len(), 3);
}

#[test]
fn build_plan_skips_unmapped_tabs() {
    let stashes = HashMap::from([("9".to_string(), vec![item("x", 0, json!({}))])]);
    let lookup = |_item: &Value| ok(1000);
    let call = PlanCall { tab_mapping: vec![4, 20, 5, 6, 7, 8, 0, 30], ..Default::default() };
    let result = plan(&stashes, Some(&lookup), &rules(&["9"]), call).unwrap();
    assert!(result.entries.is_empty());
    assert_eq!(result.skipped[0].reason, "stash tab not mapped in DnDTools settings");
}

#[test]
fn build_plan_explains_unmapped_tabs_and_empty_plan() {
    let stashes = HashMap::from([("4".to_string(), vec![item("x", 0, json!({}))])]);
    let lookup = |_item: &Value| ok(1000);
    let call = PlanCall { tab_mapping: vec![0; 8], ..Default::default() };
    let result = plan(&stashes, Some(&lookup), &rules(&["4"]), call).unwrap();
    assert!(result.entries.is_empty());
    assert!(result.warnings.iter().any(|w| w.contains("Stash Tab Mapping")));
    assert!(result.warnings.iter().any(|w| w.contains("No items to list")));
}

#[test]
fn build_plan_has_no_empty_warning_when_items_found() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({}))])]);
    let lookup = |_item: &Value| ok(1000);
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), PlanCall::default()).unwrap();
    assert!(!result.warnings.iter().any(|w| w.contains("No items to list")));
}

#[test]
fn build_plan_warns_on_stale_data() {
    let stashes = HashMap::from([("2".to_string(), vec![])]);
    let lookup = |_item: &Value| ok(1000);
    let call = PlanCall { data_age_s: Some(900.0), ..Default::default() };
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), call).unwrap();
    assert!(result.warnings.iter().any(|w| w.contains("minutes old")));
}

#[test]
fn build_plan_missing_key_raises() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({}))])]);
    let lookup = |_item: &Value| json!({"success": false, "error_code": "missing_api_key"});
    let err = plan(&stashes, Some(&lookup), &rules(&["2"]), PlanCall::default()).unwrap_err();
    assert_eq!(err.code, "missing_api_key");
}

#[test]
fn build_plan_rate_limited_returns_partial() {
    let stashes =
        HashMap::from([("2".to_string(), vec![item("a", 0, json!({})), item("b", 1, json!({})), item("c", 2, json!({}))])]);
    let calls = std::cell::RefCell::new(Vec::new());
    let lookup = |item: &Value| {
        calls.borrow_mut().push(item["itemUniqueId"].as_str().unwrap().to_string());
        if calls.borrow().len() == 1 { ok(1000) } else { json!({"success": false, "error_code": "rate_limited"}) }
    };
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), PlanCall::default()).unwrap();
    assert_eq!(result.entries.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["a"]);
    assert_eq!(*calls.borrow(), vec!["a", "b"]);
    assert!(result.warnings.iter().any(|w| w.to_lowercase().contains("rate limit")));
}

#[test]
fn plan_entry_round_trip_and_validation() {
    let entry = PlanEntry::new("a", "Item", 5, "2", 3, 1, 1, 900, 45, 10);
    assert_eq!(PlanEntry::from_dict(&entry.to_dict(), false).unwrap(), entry);
    for bad in [json!(0), json!(-5), json!("abc"), json!(MAX_LISTING_PRICE + 1), Value::Null, json!(12.5)] {
        let mut data = entry.to_dict();
        data["price"] = bad;
        assert!(matches!(PlanEntry::from_dict(&data, false), Err(PlanEntryError::NotAWholeNumber { .. } | PlanEntryError::OutOfRange { .. })));
    }
    let mut bad_slot = entry.to_dict();
    bad_slot["slot_id"] = json!(-1);
    assert!(PlanEntry::from_dict(&bad_slot, false).is_err());
}

#[test]
fn build_plan_skips_already_listed_unique_ids() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 0, json!({})), item("b", 1, json!({}))])]);
    let looked_up = std::cell::RefCell::new(Vec::new());
    let lookup = |item: &Value| {
        looked_up.borrow_mut().push(item["itemUniqueId"].as_str().unwrap().to_string());
        ok(1000)
    };
    let call = PlanCall { exclude_unique_ids: HashSet::from(["a".to_string()]), ..Default::default() };
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), call).unwrap();
    assert_eq!(result.entries.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["b"]);
    assert_eq!(
        result.skipped.iter().map(|s| (s.name.as_str(), s.reason.as_str())).collect::<Vec<_>>(),
        [("Item a", "already listed")]
    );
    assert_eq!(*looked_up.borrow(), vec!["b"]);
}

#[test]
fn build_plan_without_price_lookup_defers_pricing_to_the_game() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 3, json!({}))])]);
    let result = plan(&stashes, None, &rules(&["2"]), PlanCall::default()).unwrap();
    let entry = &result.entries[0];
    assert_eq!((entry.unique_id.as_str(), entry.price, entry.fee, entry.item_id.as_str()), ("a", 0, 0, "Id_a"));
    assert!(result.warnings.iter().any(|w| w.contains("in-game market")));
}

/// DarkerDB quotes one unit, not the stack: a stackable item must always be deferred to the
/// in-game market, never priced by multiplying a single-unit DarkerDB quote.
#[test]
fn darkerdb_pricing_never_prices_a_stack_as_one_unit() {
    let stashes = HashMap::from([("2".to_string(), vec![item("pots", 0, json!({"itemCount": 5, "max_stack_size": 5}))])]);
    let stack_rules = ListerRules { allow_stacks: true, ..rules(&["2"]) };
    let lookup = |_item: &Value| ok(100);
    let result = plan(&stashes, Some(&lookup), &stack_rules, PlanCall::default()).unwrap();
    assert!(result.entries.is_empty());
    assert!(result.skipped[0].reason.contains("Price from game"));
}

#[test]
fn darkerdb_prices_remember_the_recommendation_so_edits_are_kept() {
    let stashes = HashMap::from([("2".to_string(), vec![item("a", 3, json!({}))])]);
    let lookup = |_item: &Value| ok(1000);
    let result = plan(&stashes, Some(&lookup), &rules(&["2"]), PlanCall::default()).unwrap();
    assert_eq!((result.entries[0].price, result.entries[0].recommended), (900, 900));
}
