//! Marketplace rules the game enforces, plus the auto lister's own settings and item filtering.
//!
//! Port of DnDTools' `src/models/market_rules.py`.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Listing an item costs this share of its price, rounded up, and at least `LISTING_FEE_MIN`; the
/// fee is not refunded.
pub const LISTING_FEE_RATE: f64 = 0.05;
pub const LISTING_FEE_MIN: i64 = 15;

/// What listing at `price` costs.
pub fn listing_fee(price: i64) -> i64 {
    LISTING_FEE_MIN.max((price as f64 * LISTING_FEE_RATE).ceil() as i64)
}

/// The stash tab the game calls "Inventory".
pub const INVENTORY_STASH_ID: &str = "2";
/// An undercut can never ask this large a discount off the reference price.
pub const MAX_UNDERCUT_PCT: f64 = 90.0;
/// A lowest ask below this fraction of the average price is treated as a lone lowball, not a
/// reference (see [`reference_price`]).
pub const LOWEST_ASK_MIN_RATIO: f64 = 0.5;
/// Gold coins and their containers (bags, purses, pouches, chests) plus silver are currency.
pub const CURRENCY_ITEM_PREFIXES: [&str; 2] = ["GoldCoin", "SilverCoin"];

fn rarity_id_from_name(name: &str) -> i64 {
    match name {
        "poor" => 1,
        "common" => 2,
        "uncommon" => 3,
        "rare" => 4,
        "epic" => 5,
        "legend" | "legendary" => 6,
        "unique" => 7,
        "artifact" => 8,
        _ => 0,
    }
}

/// A rarity as a name ("Epic"), an id, or anything else (0). Case- and whitespace-insensitive.
pub fn rarity_id(value: &Value) -> i64 {
    match value {
        Value::Bool(_) | Value::Null => 0,
        Value::Number(n) => n.as_i64().unwrap_or(0),
        Value::String(s) => rarity_id_from_name(s.trim().to_lowercase().as_str()),
        _ => 0,
    }
}

/// Python-like `str(value)` for the JSON scalars item and settings data actually uses.
fn json_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Null => "None".to_string(),
        other => other.to_string(),
    }
}

/// Python truthiness for a decoded JSON value (`0`, `""`, `null`, `false`, `[]` and `{}` are falsy).
fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `dict.get(key, default)`: falls back only when the key is absent, then stringifies.
fn get_str(item: &Value, key: &str, default: &str) -> String {
    item.get(key).map_or_else(|| default.to_string(), json_str)
}

/// `dict.get(key) or default`: falls back when absent OR falsy, then stringifies.
fn get_str_or(item: &Value, key: &str, default: &str) -> String {
    match item.get(key) {
        Some(v) if json_truthy(v) => json_str(v),
        _ => default.to_string(),
    }
}

/// `int(dict.get(key, default))`: falls back only when the key is absent.
fn get_i64(item: &Value, key: &str, default: i64) -> i64 {
    match item.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(default),
        _ => default,
    }
}

/// `int(dict.get(key) or default)`: falls back when absent OR falsy (e.g. `0`).
fn get_i64_or(item: &Value, key: &str, default: i64) -> i64 {
    match item.get(key) {
        Some(v) if json_truthy(v) => match v {
            Value::Number(n) => n.as_i64().unwrap_or(default),
            _ => default,
        },
        _ => default,
    }
}

/// `float(value)` clamped to `[low, high]`, or `default` when `value` isn't a real number.
fn clamp_f64(value: Option<&Value>, low: f64, high: f64, default: f64) -> f64 {
    let number = match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    number.map_or(default, |n| n.max(low).min(high))
}

/// Where prices come from: search in game, saved market data, or the value formula.
pub const PRICE_SOURCES: [&str; 3] = ["live", "database", "model"];

/// The auto lister's settings: what to sell, how aggressively to undercut, and where prices come
/// from. Immutable, like the Python `frozen=True` dataclass it ports — build a changed copy with
/// struct-update syntax (`ListerRules { min_price: 50, ..rules }`) rather than mutating in place.
///
/// `Serialize`/`Deserialize` (camelCase) give the app's own JSON settings storage a direct,
/// strict mapping. [`ListerRules::from_dict`] is the separate, lenient entry point that mirrors
/// the Python settings dict exactly: unknown shapes, missing fields and out-of-range values are
/// clamped to sane defaults instead of failing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListerRules {
    pub source_stash_ids: Vec<String>,
    pub min_rarity: i64,
    pub min_price: i64,
    pub undercut_pct: f64,
    pub min_listings: i64,
    pub max_items_per_run: i64,
    pub exclude_item_ids: BTreeSet<String>,
    /// Net proceeds (price minus fee) must be at least this fraction of the price.
    pub min_net_ratio: f64,
    /// List stackable items (priced per unit x stack size).
    pub allow_stacks: bool,
    /// Where prices come from, see [`PRICE_SOURCES`].
    pub price_source: String,
}

impl Default for ListerRules {
    fn default() -> Self {
        ListerRules {
            source_stash_ids: vec![INVENTORY_STASH_ID.to_string()],
            min_rarity: 4,
            min_price: 100,
            undercut_pct: 10.0,
            min_listings: 3,
            max_items_per_run: 20,
            exclude_item_ids: BTreeSet::new(),
            min_net_ratio: 0.5,
            allow_stacks: false,
            price_source: "live".to_string(),
        }
    }
}

impl ListerRules {
    /// Builds settings from an untrusted JSON settings object the same way the Python
    /// `ListerRules.from_dict` does: a non-object, missing fields, or values of the wrong shape
    /// all fall back to defaults rather than erroring, and every numeric field is clamped to a
    /// sane range.
    pub fn from_dict(data: &Value) -> Self {
        let default = Self::default();
        let get = |key: &str| data.as_object().and_then(|m| m.get(key));

        let source_stash_ids = match get("source_stash_ids").and_then(Value::as_array).filter(|a| !a.is_empty()) {
            Some(arr) => arr.iter().map(json_str).collect(),
            None => default.source_stash_ids.clone(),
        };
        let min_rarity = {
            // A rarity of 0 (unrecognised or explicitly 0) falls back to the default, same as
            // Python's `rarity_id(...) or d.min_rarity`.
            let parsed = get("min_rarity").map(rarity_id).unwrap_or(default.min_rarity);
            if parsed == 0 { default.min_rarity } else { parsed }
        };
        let min_price = clamp_f64(get("min_price"), 0.0, 10_000_000.0, default.min_price as f64) as i64;
        let undercut_pct = clamp_f64(get("undercut_pct"), 0.0, MAX_UNDERCUT_PCT, default.undercut_pct);
        let min_listings = clamp_f64(get("min_listings"), 0.0, 1000.0, default.min_listings as f64) as i64;
        let max_items_per_run = clamp_f64(get("max_items_per_run"), 1.0, 40.0, default.max_items_per_run as f64) as i64;
        let exclude_item_ids = get("exclude_item_ids")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
            .map(|arr| arr.iter().map(json_str).collect())
            .unwrap_or_default();
        let min_net_ratio = clamp_f64(get("min_net_ratio"), 0.0, 1.0, default.min_net_ratio);
        let allow_stacks = matches!(get("allow_stacks"), Some(Value::Bool(true)));
        let price_source = get("price_source")
            .and_then(Value::as_str)
            .filter(|s| PRICE_SOURCES.contains(s))
            .map(str::to_owned)
            .unwrap_or(default.price_source);

        ListerRules {
            source_stash_ids,
            min_rarity,
            min_price,
            undercut_pct,
            min_listings,
            max_items_per_run,
            exclude_item_ids,
            min_net_ratio,
            allow_stacks,
            price_source,
        }
    }

    /// The Python settings dict shape (snake_case keys, `exclude_item_ids` sorted), for round
    /// tripping through [`ListerRules::from_dict`] and for compatibility with settings written by
    /// the original app.
    pub fn to_dict(&self) -> Value {
        serde_json::json!({
            "source_stash_ids": self.source_stash_ids,
            "min_rarity": self.min_rarity,
            "min_price": self.min_price,
            "undercut_pct": self.undercut_pct,
            "min_listings": self.min_listings,
            "max_items_per_run": self.max_items_per_run,
            "exclude_item_ids": self.exclude_item_ids,
            "min_net_ratio": self.min_net_ratio,
            "allow_stacks": self.allow_stacks,
            "price_source": self.price_source,
        })
    }
}

/// Skip reasons that mean a merchant is the better buyer (the page offers these to "Sell to
/// merchant").
pub const MERCHANT_REASONS: [&str; 3] = ["vendor pays more", "below min price", "below minimum rarity"];
pub const MERCHANT_REASON_PREFIX: &str = "a merchant sells it for";

pub fn is_merchant_reason(reason: &str) -> bool {
    MERCHANT_REASONS.contains(&reason) || reason.starts_with(MERCHANT_REASON_PREFIX)
}

/// An item skipped by the lister, with why.
#[derive(Debug, Clone, PartialEq)]
pub struct Skip {
    pub name: String,
    pub stash_id: String,
    pub slot_id: i64,
    pub reason: String,
    /// A doubt about the price that led to the skip.
    pub flag: String,
    pub confidence: String,
    /// The item's itemUniqueId, so a skipped item can still be sold to a merchant.
    pub unique_id: String,
}

/// A stash item worth pricing: its source stash and the raw item JSON as read from the game.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub stash_id: String,
    pub item: Value,
}

fn is_currency(item_id: &str) -> bool {
    CURRENCY_ITEM_PREFIXES.iter().any(|prefix| item_id.starts_with(prefix))
}

/// Why an item can't be listed, or `None` when it's a candidate.
fn skip_reason(item: &Value, rules: &ListerRules) -> Option<&'static str> {
    if is_currency(&get_str(item, "itemId", "")) {
        return Some("gold is never listed");
    }
    if get_i64_or(item, "max_stack_size", 1) > 1 && !rules.allow_stacks {
        return Some("stacks are left out (turn on Include stacks)");
    }
    if rules.exclude_item_ids.contains(&get_str(item, "itemId", "")) {
        return Some("on your never-sell list");
    }
    if item.get("rarity").map_or(0, rarity_id) < rules.min_rarity {
        return Some("below minimum rarity");
    }
    None
}

/// Splits the source stashes into listable candidates and skipped items (with reasons), ordered
/// by source stash then slot.
pub fn select_candidates(stashes: &HashMap<String, Vec<Value>>, rules: &ListerRules) -> (Vec<Candidate>, Vec<Skip>) {
    let mut candidates = Vec::new();
    let mut skipped = Vec::new();
    for stash_id in &rules.source_stash_ids {
        let mut items: Vec<&Value> = stashes.get(stash_id).map_or_else(Vec::new, |v| v.iter().collect());
        items.sort_by_key(|item| get_i64(item, "slotId", 0));
        for item in items {
            match skip_reason(item, rules) {
                Some(reason) => skipped.push(Skip {
                    name: get_str(item, "name", "?"),
                    stash_id: stash_id.clone(),
                    slot_id: get_i64(item, "slotId", 0),
                    reason: reason.to_string(),
                    flag: String::new(),
                    confidence: String::new(),
                    unique_id: get_str_or(item, "itemUniqueId", ""),
                }),
                None => candidates.push(Candidate { stash_id: stash_id.clone(), item: item.clone() }),
            }
        }
    }
    (candidates, skipped)
}

/// The outcome of pricing an item from a saved (non-live) price check.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceDecision {
    pub ok: bool,
    pub price: Option<i64>,
    pub fee: i64,
    pub reason: String,
}

fn no(reason: &str) -> PriceDecision {
    PriceDecision { ok: false, price: None, fee: 0, reason: reason.to_string() }
}

fn positive_number(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(n)) => n.as_f64().filter(|v| *v > 0.0),
        _ => None,
    }
}

/// The cheapest of the lowest ask and the average price, ignoring a lowest ask far below the
/// average — a lone lowball listing shouldn't drag our price down.
fn reference_price(price_check: &Value) -> Option<f64> {
    let mut lowest = positive_number(price_check.get("lowest_ask"));
    let avg = positive_number(price_check.get("avg_price"));
    if let (Some(l), Some(a)) = (lowest, avg) {
        if l < LOWEST_ASK_MIN_RATIO * a {
            lowest = None;
        }
    }
    match (lowest, avg) {
        (Some(l), Some(a)) => Some(l.min(a)),
        (Some(l), None) => Some(l),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

/// Prices an item from a saved DarkerDB-style price check (`{success, has_data, avg_price,
/// lowest_ask, num_listings}`), undercutting the reference price and refusing to list when the
/// data is thin, the price is below the floor, or the fee eats the margin.
pub fn compute_price(price_check: Option<&Value>, vendor_price: i64, rules: &ListerRules) -> PriceDecision {
    let ready = price_check
        .is_some_and(|pc| pc.get("success").is_some_and(json_truthy) && pc.get("has_data").is_some_and(json_truthy));
    if !ready {
        return no("no market data");
    }
    let price_check = price_check.expect("ready is true only when price_check is Some");
    let num_listings = price_check.get("num_listings").and_then(Value::as_i64).unwrap_or(0);
    if num_listings < rules.min_listings {
        return no("not enough market data");
    }
    let reference = match reference_price(price_check) {
        Some(r) => r,
        None => return no("no market data"),
    };
    let price = (reference * (1.0 - rules.undercut_pct / 100.0)).floor() as i64;
    if price < rules.min_price.max(1) {
        return no("below min price");
    }
    let fee = listing_fee(price);
    let net = price - fee;
    if net <= vendor_price {
        return no("vendor pays more");
    }
    if (net as f64 / price as f64) < rules.min_net_ratio {
        return no("fee too high");
    }
    PriceDecision { ok: true, price: Some(price), fee, reason: "ok".to_string() }
}
