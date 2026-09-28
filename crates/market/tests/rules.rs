//! Port of DnDTools' `tests/test_market_rules.py`.

use std::collections::{HashMap, HashSet};

use market::{compute_price, listing_fee, rarity_id, select_candidates, Candidate, ListerRules};
use serde_json::{json, Value};

fn item(overrides: Value) -> Value {
    let mut base = json!({
        "name": "Riveted Gloves", "itemId": "RivetedGloves_5001", "itemUniqueId": "111",
        "slotId": 3, "itemCount": 1, "rarity": 5, "width": 2, "height": 2,
        "pp": [], "sp": [], "vendor_price": 20, "max_stack_size": 1,
    });
    if let (Value::Object(base_map), Value::Object(over_map)) = (&mut base, overrides) {
        for (k, v) in over_map {
            base_map.insert(k, v);
        }
    }
    base
}

fn check(overrides: Value) -> Value {
    let mut base = json!({"success": true, "has_data": true, "avg_price": 1000, "lowest_ask": 900, "num_listings": 12});
    if let (Value::Object(base_map), Value::Object(over_map)) = (&mut base, overrides) {
        for (k, v) in over_map {
            base_map.insert(k, v);
        }
    }
    base
}

fn item_id(candidate: &Candidate) -> &str {
    candidate.item["itemUniqueId"].as_str().unwrap()
}

#[test]
fn listing_fee_has_15_gold_minimum() {
    assert_eq!(listing_fee(100), 15);
    assert_eq!(listing_fee(500), 25);
    assert_eq!(listing_fee(301), 16); // ceil(15.05)
}

#[test]
fn rarity_id_accepts_names_and_ints() {
    assert_eq!(rarity_id(&json!(5)), 5);
    assert_eq!(rarity_id(&json!("Epic")), 5);
    assert_eq!(rarity_id(&json!("legend")), 6);
    assert_eq!(rarity_id(&Value::Null), 0);
    assert_eq!(rarity_id(&json!("nonsense")), 0);
}

#[test]
fn rules_from_dict_clamps_and_defaults() {
    let rules = ListerRules::from_dict(&json!({
        "undercut_pct": 150, "max_items_per_run": 0, "min_rarity": "Rare",
        "source_stash_ids": [2, "4"], "exclude_item_ids": ["A"],
    }));
    assert_eq!(rules.undercut_pct, 90.0);
    assert_eq!(rules.max_items_per_run, 1);
    assert_eq!(rules.min_rarity, 4);
    assert_eq!(rules.source_stash_ids, vec!["2".to_string(), "4".to_string()]);
    assert_eq!(rules.exclude_item_ids, std::collections::BTreeSet::from(["A".to_string()]));
    assert_eq!(ListerRules::from_dict(&rules.to_dict()), rules);
}

#[test]
fn select_candidates_filters_with_reasons() {
    let first = item(json!({}));
    let mut stashes = HashMap::new();
    stashes.insert(
        "2".to_string(),
        vec![
            first.clone(),
            item(json!({"itemUniqueId": "222", "rarity": 2, "slotId": 5})),
            item(json!({"itemUniqueId": "333", "max_stack_size": 5, "slotId": 7})),
        ],
    );
    stashes.insert("4".to_string(), vec![item(json!({"itemUniqueId": "444", "itemId": "Excluded_1", "slotId": 0}))]);
    stashes.insert("5".to_string(), vec![item(json!({"itemUniqueId": "555"}))]);
    let rules = ListerRules {
        source_stash_ids: vec!["2".to_string(), "4".to_string()],
        exclude_item_ids: std::collections::BTreeSet::from(["Excluded_1".to_string()]),
        ..ListerRules::default()
    };
    let (candidates, skipped) = select_candidates(&stashes, &rules);
    assert_eq!(candidates, vec![Candidate { stash_id: "2".to_string(), item: first }]);
    let reasons: HashMap<i64, String> = skipped.iter().map(|s| (s.slot_id, s.reason.clone())).collect();
    assert_eq!(
        reasons,
        HashMap::from([
            (5, "below minimum rarity".to_string()),
            (7, "stacks are left out (turn on Include stacks)".to_string()),
            (0, "on your never-sell list".to_string()),
        ])
    );
}

#[test]
fn select_candidates_never_lists_currency() {
    let mut stashes = HashMap::new();
    stashes.insert(
        "4".to_string(),
        vec![
            item(json!({"itemUniqueId": "bag", "itemId": "GoldCoinBag", "slotId": 0})),
            item(json!({"itemUniqueId": "purse", "itemId": "GoldCoinPurse", "slotId": 1})),
            item(json!({"itemUniqueId": "pouch", "itemId": "GoldCoinPouch", "slotId": 2})),
            item(json!({"itemUniqueId": "ring", "itemId": "GemRing_5001", "slotId": 3})),
        ],
    );
    let rules = ListerRules { source_stash_ids: vec!["4".to_string()], ..ListerRules::default() };
    let (candidates, skipped) = select_candidates(&stashes, &rules);
    assert_eq!(candidates.iter().map(item_id).collect::<Vec<_>>(), vec!["ring"]);
    let reasons: HashSet<&str> = skipped.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(reasons, HashSet::from(["gold is never listed"]));
}

#[test]
fn select_candidates_orders_by_source_then_slot() {
    let mut stashes = HashMap::new();
    stashes.insert(
        "4".to_string(),
        vec![item(json!({"itemUniqueId": "b", "slotId": 9})), item(json!({"itemUniqueId": "a", "slotId": 1}))],
    );
    stashes.insert("2".to_string(), vec![item(json!({"itemUniqueId": "c", "slotId": 4}))]);
    let rules = ListerRules { source_stash_ids: vec!["2".to_string(), "4".to_string()], ..ListerRules::default() };
    let (candidates, _) = select_candidates(&stashes, &rules);
    assert_eq!(candidates.iter().map(item_id).collect::<Vec<_>>(), vec!["c", "a", "b"]);
}

#[test]
fn compute_price_undercuts_lower_reference() {
    let decision = compute_price(Some(&check(json!({}))), 20, &ListerRules::default());
    assert!(decision.ok && decision.price == Some(810) && decision.fee == 41); // 900 * 0.9
}

#[test]
fn compute_price_skips_without_data() {
    assert_eq!(compute_price(None, 0, &ListerRules::default()).reason, "no market data");
    assert_eq!(compute_price(Some(&check(json!({"success": false}))), 0, &ListerRules::default()).reason, "no market data");
    assert_eq!(
        compute_price(Some(&check(json!({"num_listings": 1}))), 0, &ListerRules::default()).reason,
        "not enough market data"
    );
}

#[test]
fn compute_price_ignores_zero_reference_prices() {
    assert_eq!(
        compute_price(Some(&check(json!({"lowest_ask": 0, "avg_price": null}))), 0, &ListerRules::default()).reason,
        "no market data"
    );
    // Falls back to avg 1000.
    assert_eq!(compute_price(Some(&check(json!({"lowest_ask": 0}))), 0, &ListerRules::default()).price, Some(900));
}

#[test]
fn compute_price_skip_reasons() {
    let rules = ListerRules { min_price: 100, ..ListerRules::default() };
    assert_eq!(
        compute_price(Some(&check(json!({"lowest_ask": 90, "avg_price": 90}))), 0, &rules).reason,
        "below min price"
    );
    assert_eq!(compute_price(Some(&check(json!({}))), 900, &rules).reason, "vendor pays more");
    let cheap = ListerRules { min_price: 1, min_net_ratio: 0.5, ..ListerRules::default() };
    // 27 - 15
    assert_eq!(compute_price(Some(&check(json!({"lowest_ask": 30, "avg_price": 30}))), 0, &cheap).reason, "fee too high");
}

#[test]
fn compute_price_ignores_outlier_low_lowest_ask() {
    // Priced off avg.
    assert_eq!(
        compute_price(Some(&check(json!({"lowest_ask": 100, "avg_price": 1000}))), 0, &ListerRules::default()).price,
        Some(900)
    );
    // Exactly 0.5 kept.
    assert_eq!(
        compute_price(Some(&check(json!({"lowest_ask": 500, "avg_price": 1000}))), 0, &ListerRules::default()).price,
        Some(450)
    );
    let rules = ListerRules { min_price: 1, ..ListerRules::default() };
    assert_eq!(compute_price(Some(&check(json!({"lowest_ask": 100, "avg_price": null}))), 0, &rules).price, Some(90));
}

#[test]
fn stacks_are_listed_only_when_allowed() {
    let mut stashes = HashMap::new();
    stashes.insert("2".to_string(), vec![item(json!({"itemUniqueId": "pots", "max_stack_size": 3, "slotId": 0}))]);
    assert_eq!(select_candidates(&stashes, &ListerRules::default()).0, vec![]);
    let allow = ListerRules { allow_stacks: true, ..ListerRules::default() };
    let (candidates, _) = select_candidates(&stashes, &allow);
    assert_eq!(candidates.iter().map(item_id).collect::<Vec<_>>(), vec!["pots"]);
    assert!(ListerRules::from_dict(&json!({"allow_stacks": true})).allow_stacks);
    assert!(!ListerRules::from_dict(&json!({"allow_stacks": "yes"})).allow_stacks);
}

#[test]
fn price_source_setting_accepts_live_database_or_formula() {
    assert_eq!(ListerRules::default().price_source, "live");
    for source in ["live", "database", "model"] {
        let rules = ListerRules::from_dict(&json!({"price_source": source}));
        assert_eq!(rules.price_source, source);
        assert_eq!(ListerRules::from_dict(&rules.to_dict()), rules);
    }
    assert_eq!(ListerRules::from_dict(&json!({"price_source": "guess"})).price_source, "live");
}
