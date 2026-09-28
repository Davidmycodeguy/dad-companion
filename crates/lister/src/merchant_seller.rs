//! Pure rules for selling the lister's leftovers to a merchant (no I/O).
//!
//! Port of DnDTools' `src/models/merchant_seller.py`.

use std::collections::{HashMap, HashSet};

use market::Candidate;
use serde_json::Value;

use crate::plan::{get_str_or, plan_entry, str_of, PlanEntry};

/// The merchant's Sell box, in cells.
pub const SELL_BOX_COLUMNS: i64 = 10;
pub const SELL_BOX_ROWS: i64 = 6;
pub const GOLD_REFUSAL: &str = "gold is never sold";
pub const MISSING_REFUSAL: &str = "not found in the chosen stash tabs";
pub const NOT_TRADABLE_REFUSAL: &str = "not tradable — usually a quest item, so it's kept (sell it by hand if you're sure)";

/// Tradable items carry `tradable == 1`; quest items (e.g. Huntress' emblems) have no flag. Python's
/// `== 1` also accepts the JSON boolean `true` (a `bool` is an `int` subclass there); matched here
/// for the same tolerance even though real captures use the numeric form.
fn is_tradable(item: &Value) -> bool {
    match item.get("originalData").and_then(|d| d.get("tradable")) {
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        Some(Value::Bool(b)) => *b,
        _ => false,
    }
}

/// Gold a merchant pays for the whole stack.
pub fn merchant_value(entry: &PlanEntry) -> i64 {
    entry.vendor_price * entry.quantity
}

/// Stash ids the app must never touch are dropped from a caller-supplied tab list before it is
/// searched at all — defence in depth beyond whatever already keeps the locked Seasonal Shared
/// Stash out of `allowed_stash_ids` in the first place.
fn drop_off_limits_ids(stash_ids: &[String]) -> Vec<String> {
    stash_ids.iter().filter(|id| id.parse::<u32>().is_ok_and(|n| !state::is_off_limits(n))).cloned().collect()
}

/// `(entries, refused)` for the requested items as they sit in the stash right now.
///
/// Positions always come from the stash data, never from the caller. `refused` holds
/// `(unique_id, reason)` for gold, non-tradable items and items not in the allowed tabs.
pub fn resolve_sell_entries(
    stashes: &HashMap<String, Vec<Value>>,
    unique_ids: &[String],
    allowed_stash_ids: &[String],
) -> (Vec<PlanEntry>, Vec<(String, String)>) {
    let mut found: HashMap<String, (String, &Value)> = HashMap::new();
    for stash_id in drop_off_limits_ids(allowed_stash_ids) {
        for item in stashes.get(&stash_id).map_or([].as_slice(), Vec::as_slice) {
            found.entry(str_of(item, "itemUniqueId")).or_insert((stash_id.clone(), item));
        }
    }
    let mut entries = Vec::new();
    let mut refused = Vec::new();
    let mut seen = HashSet::new();
    for raw_id in unique_ids {
        if !seen.insert(raw_id.clone()) {
            continue;
        }
        let Some((stash_id, item)) = found.get(raw_id) else {
            refused.push((raw_id.clone(), MISSING_REFUSAL.to_string()));
            continue;
        };
        let item_id = get_str_or(item, "itemId", "", usize::MAX);
        if market::CURRENCY_ITEM_PREFIXES.iter().any(|p| item_id.starts_with(p)) {
            refused.push((raw_id.clone(), GOLD_REFUSAL.to_string()));
            continue;
        }
        if !is_tradable(item) {
            refused.push((raw_id.clone(), NOT_TRADABLE_REFUSAL.to_string()));
            continue;
        }
        entries.push(plan_entry(&Candidate { stash_id: stash_id.clone(), item: (*item).clone() }, None));
    }
    (entries, refused)
}

/// Where one item goes in the Sell box (its top-left cell).
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub entry: PlanEntry,
    pub col: i64,
    pub row: i64,
}

fn cells(col: i64, row: i64, width: i64, height: i64) -> HashSet<(i64, i64)> {
    (0..width).flat_map(|dx| (0..height).map(move |dy| (col + dx, row + dy))).collect()
}

fn first_fit(occupied: &HashSet<(i64, i64)>, width: i64, height: i64, columns: i64, rows: i64) -> Option<(i64, i64)> {
    for row in 0..=(rows - height) {
        for col in 0..=(columns - width) {
            if cells(col, row, width, height).is_disjoint(occupied) {
                return Some((col, row));
            }
        }
    }
    None
}

/// `(batches, too_big)`: each batch fills one Sell box without overlaps, in the given order.
///
/// Items that don't fit wait for the next batch; items larger than an empty box are `too_big`.
pub fn pack_sell_box(entries: Vec<PlanEntry>, columns: i64, rows: i64) -> (Vec<Vec<Placement>>, Vec<PlanEntry>) {
    let (too_big, mut pending): (Vec<PlanEntry>, Vec<PlanEntry>) =
        entries.into_iter().partition(|e| e.width > columns || e.height > rows);
    let mut batches = Vec::new();
    while !pending.is_empty() {
        let mut occupied = HashSet::new();
        let mut batch = Vec::new();
        let mut rest = Vec::new();
        for entry in pending {
            match first_fit(&occupied, entry.width, entry.height, columns, rows) {
                Some((col, row)) => {
                    occupied.extend(cells(col, row, entry.width, entry.height));
                    batch.push(Placement { entry, col, row });
                }
                None => rest.push(entry),
            }
        }
        batches.push(batch);
        pending = rest;
    }
    (batches, too_big)
}

/// The result of a merchant sale run.
#[derive(Debug, Clone, PartialEq)]
pub struct SaleOutcome {
    /// Entries the merchant took.
    pub sold: Vec<PlanEntry>,
    /// Staged entries still in the stash.
    pub not_taken: Vec<PlanEntry>,
    /// Unique ids sold that were never staged (a wrong item was dragged).
    pub unexpected: Vec<String>,
}

pub fn sale_outcome(staged: &[PlanEntry], deleted_ids: &[String]) -> SaleOutcome {
    let deleted: HashSet<&String> = deleted_ids.iter().collect();
    let staged_ids: HashSet<&String> = staged.iter().map(|e| &e.unique_id).collect();
    let mut unexpected: Vec<String> = deleted.difference(&staged_ids).map(|s| (*s).clone()).collect();
    unexpected.sort();
    SaleOutcome {
        sold: staged.iter().filter(|e| deleted.contains(&e.unique_id)).cloned().collect(),
        not_taken: staged.iter().filter(|e| !deleted.contains(&e.unique_id)).cloned().collect(),
        unexpected,
    }
}
