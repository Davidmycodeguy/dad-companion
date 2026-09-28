//! Port of DnDTools' `tests/test_merchant_seller.py` (`resolve_sell_entries`, `pack_sell_box` and
//! `sale_outcome`; `is_merchant_reason` itself is `market`'s function, tested there).

mod common;

use std::collections::HashMap;

use lister::merchant_seller::{merchant_value, pack_sell_box, resolve_sell_entries, sale_outcome, SELL_BOX_COLUMNS, SELL_BOX_ROWS};
use lister::plan::PlanEntry;
use serde_json::{json, Value};

/// The merchant-seller tests' item shape: DnDTools' `_item` helper here (unlike the plan tests')
/// defaults to tradable, since tradability is exactly what most of these tests are about.
fn item(uid: &str, slot: i64, overrides: Value) -> Value {
    let mut base = common::item(uid, slot, json!({"originalData": {"tradable": 1}}));
    if let (Value::Object(b), Value::Object(o)) = (&mut base, overrides) {
        b.extend(o);
    }
    base
}

fn entry(uid: &str) -> PlanEntry {
    PlanEntry::new(uid, format!("Item {uid}"), 3, "4", 0, 1, 1, 0, 0, 10)
}

#[test]
fn resolve_finds_items_by_unique_id_at_their_current_slot() {
    let stashes = HashMap::from([
        ("4".to_string(), vec![item("a", 34, json!({"vendor_price": 250}))]),
        ("20".to_string(), vec![item("b", 154, json!({"width": 1, "height": 2, "vendor_price": 100}))]),
    ]);
    let (entries, refused) =
        resolve_sell_entries(&stashes, &["b".to_string(), "a".to_string()], &["4".to_string(), "20".to_string()]);
    let seen: Vec<(&str, &str, i64, i64, i64, i64)> =
        entries.iter().map(|e| (e.unique_id.as_str(), e.stash_id.as_str(), e.slot_id, e.width, e.height, e.vendor_price)).collect();
    assert_eq!(seen, [("b", "20", 154, 1, 2, 100), ("a", "4", 34, 1, 1, 250)]);
    assert!(refused.is_empty());
}

#[test]
fn resolve_keeps_stack_size_for_the_merchant_value() {
    let stashes = HashMap::from([("4".to_string(), vec![item("eyes", 142, json!({"itemCount": 2, "vendor_price": 25, "max_stack_size": 5}))])]);
    let (entries, _) = resolve_sell_entries(&stashes, &["eyes".to_string()], &["4".to_string()]);
    assert_eq!(entries[0].quantity, 2);
    assert_eq!(merchant_value(&entries[0]), 50);
}

#[test]
fn resolve_refuses_gold_missing_items_and_tabs_not_chosen() {
    let stashes = HashMap::from([
        (
            "4".to_string(),
            vec![item("gold", 1, json!({"itemId": "GoldCoins", "itemCount": 25})), item("pouch", 2, json!({"itemId": "GoldCoinPouch"}))],
        ),
        ("21".to_string(), vec![item("gem", 3, json!({}))]),
    ]);
    let ids = ["gold", "pouch", "gone", "gem"].map(str::to_string);
    let (entries, refused) = resolve_sell_entries(&stashes, &ids, &["4".to_string()]);
    assert!(entries.is_empty());
    assert_eq!(
        refused,
        [
            ("gold".to_string(), "gold is never sold".to_string()),
            ("pouch".to_string(), "gold is never sold".to_string()),
            ("gone".to_string(), "not found in the chosen stash tabs".to_string()),
            ("gem".to_string(), "not found in the chosen stash tabs".to_string()),
        ]
    );
}

/// Quest items such as Huntress' emblems carry no `tradable` flag at all (verified in game:
/// "Non-tradable") — they are refused, unlike an ordinary (tradable-by-default) item.
#[test]
fn resolve_keeps_items_that_are_not_tradable() {
    let emblem = item("emblem", 12, json!({"originalData": {"permittedAreaArray": [{"type": 3}]}, "vendor_price": 25}));
    let stashes = HashMap::from([("21".to_string(), vec![emblem, item("gem", 13, json!({}))])]);
    let (entries, refused) = resolve_sell_entries(&stashes, &["emblem".to_string(), "gem".to_string()], &["21".to_string()]);
    assert_eq!(entries.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["gem"]);
    assert_eq!(refused[0].0, "emblem");
    assert!(refused[0].1.starts_with("not tradable"));
}

#[test]
fn resolve_ignores_duplicate_ids() {
    let stashes = HashMap::from([("4".to_string(), vec![item("a", 0, json!({}))])]);
    let (entries, refused) = resolve_sell_entries(&stashes, &["a".to_string(), "a".to_string()], &["4".to_string()]);
    assert_eq!(entries.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["a"]);
    assert!(refused.is_empty());
}

#[test]
fn pack_places_items_left_to_right_in_one_batch() {
    let entries = vec![entry("a"), PlanEntry { height: 2, ..entry("b") }, PlanEntry { width: 2, height: 2, ..entry("c") }];
    let (batches, too_big) = pack_sell_box(entries, SELL_BOX_COLUMNS, SELL_BOX_ROWS);
    assert!(too_big.is_empty());
    let placements: Vec<Vec<(&str, i64, i64)>> =
        batches.iter().map(|batch| batch.iter().map(|p| (p.entry.unique_id.as_str(), p.col, p.row)).collect()).collect();
    assert_eq!(placements, vec![vec![("a", 0, 0), ("b", 1, 0), ("c", 2, 0)]]);
}

#[test]
fn pack_moves_what_does_not_fit_to_the_next_batch() {
    let singles: Vec<PlanEntry> = (0..(SELL_BOX_COLUMNS * SELL_BOX_ROWS + 3)).map(|i| entry(&i.to_string())).collect();
    let (batches, too_big) = pack_sell_box(singles, SELL_BOX_COLUMNS, SELL_BOX_ROWS);
    assert_eq!(batches.iter().map(Vec::len).collect::<Vec<_>>(), [(SELL_BOX_COLUMNS * SELL_BOX_ROWS) as usize, 3]);
    assert!(too_big.is_empty());
    let last = batches[0].last().unwrap();
    assert_eq!((last.col, last.row), (SELL_BOX_COLUMNS - 1, SELL_BOX_ROWS - 1));
}

#[test]
fn pack_never_overlaps_tall_items() {
    let tall: Vec<PlanEntry> = (0..25).map(|i| PlanEntry { height: 3, ..entry(&format!("t{i}")) }).collect();
    let (batches, _) = pack_sell_box(tall, SELL_BOX_COLUMNS, SELL_BOX_ROWS);
    let mut total = 0;
    for batch in &batches {
        let mut cells = Vec::new();
        for p in batch {
            for dx in 0..p.entry.width {
                for dy in 0..p.entry.height {
                    cells.push((p.col + dx, p.row + dy));
                }
            }
        }
        let unique: std::collections::HashSet<_> = cells.iter().collect();
        assert_eq!(cells.len(), unique.len());
        assert!(cells.iter().all(|&(c, r)| (0..SELL_BOX_COLUMNS).contains(&c) && (0..SELL_BOX_ROWS).contains(&r)));
        total += batch.len();
    }
    assert_eq!(total, 25);
}

#[test]
fn pack_reports_items_bigger_than_the_box() {
    let entries = vec![PlanEntry { height: SELL_BOX_ROWS + 1, ..entry("huge") }, entry("a")];
    let (batches, too_big) = pack_sell_box(entries, SELL_BOX_COLUMNS, SELL_BOX_ROWS);
    assert_eq!(too_big.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["huge"]);
    let placements: Vec<Vec<&str>> = batches.iter().map(|b| b.iter().map(|p| p.entry.unique_id.as_str()).collect()).collect();
    assert_eq!(placements, vec![vec!["a"]]);
}

#[test]
fn pack_of_nothing_is_empty() {
    let (batches, too_big) = pack_sell_box(Vec::new(), SELL_BOX_COLUMNS, SELL_BOX_ROWS);
    assert!(batches.is_empty() && too_big.is_empty());
}

#[test]
fn sale_outcome_splits_sold_not_taken_and_unexpected() {
    let staged = vec![entry("a"), entry("b"), entry("c")];
    let deleted = ["c", "a", "zzz"].map(str::to_string);
    let outcome = sale_outcome(&staged, &deleted);
    assert_eq!(outcome.sold.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["a", "c"]);
    assert_eq!(outcome.not_taken.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["b"]);
    assert_eq!(outcome.unexpected, vec!["zzz".to_string()]);
}

/// Rust's static typing means `deleted_ids` is already `&[String]` at this boundary (it comes from
/// `SellBack::deleted_ids`, itself already stringified) — there is no separate "numeric id" shape
/// to accept the way Python's dynamically-typed `str(i) for i in deleted_ids` has to tolerate.
#[test]
fn sale_outcome_accepts_string_ids_that_look_numeric() {
    let outcome = sale_outcome(&[entry("123")], &["123".to_string()]);
    assert_eq!(outcome.sold.iter().map(|e| e.unique_id.as_str()).collect::<Vec<_>>(), ["123"]);
    assert!(outcome.unexpected.is_empty());
}
