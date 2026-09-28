//! Builds the auto market lister's reviewable listing plan.
//!
//! Port of DnDTools' `src/market_lister.py`.

use std::collections::{BTreeSet, HashMap, HashSet};

use market::{Candidate, ListerRules, MarketRow, Skip};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The game refuses to list anything above this price.
pub const MAX_LISTING_PRICE: i64 = 1_000_000;
/// Stash data older than this gets a "reopen your character" warning.
pub const STALE_DATA_SECONDS: f64 = 300.0;
pub const UNMAPPED_TAB_REASON: &str = "stash tab not mapped in DnDTools settings";
/// `stat_pairs` keeps at most this many (stat, value) pairs; the rest are silently dropped rather
/// than let a malformed payload grow a plan entry without bound.
pub const MAX_STATS: usize = 16;
pub const CONFIDENCE_LEVELS: [&str; 3] = ["high", "medium", "low"];
pub const MAX_QUANTITY: i64 = 999;
/// The tallest items (staves, long bows) take five cells.
pub const MAX_ITEM_HEIGHT: i64 = 5;
pub const NOT_PRICED_REASON: &str = "not priced — the pricing run stopped first";
pub const STACK_NEEDS_GAME_PRICING: &str =
    "stacks are priced per unit from the game market — use Price from game";
pub const ABOVE_MAX_REASON: &str = "price would be above the game's maximum listing price";
pub const MODEL_NO_DATA_REASON: &str = "no market data for the value formula";

// --- pluggable lookups, aliased so callers (and clippy) aren't faced with the raw `dyn Fn` spelling
// at every call site ---------------------------------------------------------------------------
//
// Each carries an explicit `'a` rather than being a bare `dyn Fn(...)`: a lifetime-free `type X =
// dyn Trait;` alias silently fixes X's object lifetime bound at `'static` (Rust's default for a
// `dyn Trait` not immediately wrapped in a `&`), which would forbid ever passing a closure that
// borrows anything short-lived — exactly the common case for these (e.g. a test closure capturing
// a local `RefCell`, or production code capturing a borrowed HTTP client). `&PriceLookup<'_>` ties
// the bound back to the reference's own lifetime instead, same as a bare `&dyn Fn(...)` would.

/// Prices one candidate item, DarkerDB-style (`{success, has_data, avg_price, ...}`); `None` when
/// pricing is deferred to the in-game market.
pub type PriceLookup<'a> = dyn Fn(&Value) -> Value + 'a;
/// Recent listings of an item from the local market history, widening `apply_game_prices`' view.
pub type ExtraRows<'a> = dyn Fn(&str) -> Vec<MarketRow> + 'a;
/// The Item Worth estimate (or a bare value) for a plan entry's exact rolls, if any.
pub type WorthLookup<'a> = dyn Fn(&PlanEntry) -> Option<ModelPrice> + 'a;
/// What a merchant pays for one of an item, if known — a hard ceiling on its price.
pub type MerchantPriceLookup<'a> = dyn Fn(&str) -> Option<f64> + 'a;
/// The full Item Worth estimate for a plan entry's exact rolls, if any (used where a floor and
/// confidence are required, unlike [`WorthLookup`]'s looser bare-value case).
pub type ModelWorthLookup<'a> = dyn Fn(&PlanEntry) -> Option<market::Estimate> + 'a;

// --- small JSON dict helpers -------------------------------------------------------------------
//
// Mirror the private helpers in `market::rules` (itself a port of `market_rules.py`): duplicated
// rather than exposed cross-crate for a handful of one-line functions, the same call
// `crates/input` already made for `tab_icon_index` (see below). `pub(crate)` because
// `merchant_seller` (built from the same raw stash item shape) reuses them too.

/// Python-like `str(value)` for the JSON scalars item and plan data actually carry.
pub(crate) fn json_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Null => "None".to_string(),
        other => other.to_string(),
    }
}

/// Python truthiness for a decoded JSON value (`0`, `""`, `null`, `false`, `[]` and `{}` are falsy).
pub(crate) fn json_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `str(item.get(key))`: stringifies whatever is there, or `"None"` when the key is absent —
/// exactly Python's `str(None)`.
pub(crate) fn str_of(item: &Value, key: &str) -> String {
    json_str(item.get(key).unwrap_or(&Value::Null))
}

/// `str(item.get(key, default))[:max_len]`: falls back only when the key is absent.
pub(crate) fn get_str(item: &Value, key: &str, default: &str, max_len: usize) -> String {
    let s = item.get(key).map_or_else(|| default.to_string(), json_str);
    s.chars().take(max_len).collect()
}

/// `str(item.get(key) or default)[:max_len]`: falls back when absent OR falsy.
pub(crate) fn get_str_or(item: &Value, key: &str, default: &str, max_len: usize) -> String {
    let s = match item.get(key) {
        Some(v) if json_truthy(v) => json_str(v),
        _ => default.to_string(),
    };
    s.chars().take(max_len).collect()
}

/// `int(item.get(key, default))`: falls back only when the key is absent.
pub(crate) fn get_i64(item: &Value, key: &str, default: i64) -> i64 {
    match item.get(key) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(default),
        _ => default,
    }
}

/// `int(item.get(key) or default)`: falls back when absent OR falsy (e.g. `0`).
pub(crate) fn get_i64_or(item: &Value, key: &str, default: i64) -> i64 {
    match item.get(key) {
        Some(v) if json_truthy(v) => match v {
            Value::Number(n) => n.as_i64().unwrap_or(default),
            _ => default,
        },
        _ => default,
    }
}

/// `[[stat, value], ...]` from JSON / enhanced items -> `[(stat, int), ...]`; junk is dropped.
///
/// A pair only counts when it is a 2-element array whose second element is a JSON number (`bool`
/// does not decode to `Number`, so it is excluded for free, unlike Python where `bool` is an `int`
/// subtype and needs an explicit check).
pub fn stat_pairs(raw: Option<&Value>) -> Vec<(String, i64)> {
    let Some(Value::Array(items)) = raw else { return Vec::new() };
    let mut pairs = Vec::new();
    for item in items {
        let Value::Array(pair) = item else { continue };
        let [name, value] = &pair[..] else { continue };
        let Value::Number(n) = value else { continue };
        let Some(int_value) = n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)) else { continue };
        let name: String = json_str(name).chars().take(64).collect();
        pairs.push((name, int_value));
    }
    pairs.truncate(MAX_STATS);
    pairs
}

/// One item on the plan: what it is, where it sits, and the price the player will be asked to
/// approve. Immutable, like the Python `frozen=True` dataclass it ports — build a changed copy
/// with struct-update syntax (`PlanEntry { price: 900, ..entry }`) rather than mutating in place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub unique_id: String,
    pub name: String,
    pub rarity: i64,
    pub stash_id: String,
    pub slot_id: i64,
    pub width: i64,
    pub height: i64,
    pub price: i64,
    pub fee: i64,
    pub vendor_price: i64,
    #[serde(default)]
    pub item_id: String,
    /// Primary properties of our exact copy.
    #[serde(default)]
    pub base_rolls: Vec<(String, i64)>,
    /// Random secondary properties of our exact copy.
    #[serde(default)]
    pub rolls: Vec<(String, i64)>,
    /// A pricing warning for the user to review.
    #[serde(default)]
    pub flag: String,
    /// What the price was compared against.
    #[serde(default)]
    pub compared: String,
    /// "high" | "medium" | "low" | "" — how well the market backs the price.
    #[serde(default)]
    pub confidence: String,
    /// Stack size to list (the price is for the whole stack).
    #[serde(default = "one")]
    pub quantity: i64,
    /// The price the lister computed (differs from `price` if the user edited it).
    #[serde(default)]
    pub recommended: i64,
}

fn one() -> i64 {
    1
}

impl PlanEntry {
    /// The 10 fields every entry needs; the rest take their dataclass defaults (matching the
    /// positional `PlanEntry(...)` constructor the Python tests use throughout).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        unique_id: impl Into<String>,
        name: impl Into<String>,
        rarity: i64,
        stash_id: impl Into<String>,
        slot_id: i64,
        width: i64,
        height: i64,
        price: i64,
        fee: i64,
        vendor_price: i64,
    ) -> Self {
        PlanEntry {
            unique_id: unique_id.into(),
            name: name.into(),
            rarity,
            stash_id: stash_id.into(),
            slot_id,
            width,
            height,
            price,
            fee,
            vendor_price,
            item_id: String::new(),
            base_rolls: Vec::new(),
            rolls: Vec::new(),
            flag: String::new(),
            compared: String::new(),
            confidence: String::new(),
            quantity: 1,
            recommended: 0,
        }
    }

    pub fn to_dict(&self) -> Value {
        serde_json::to_value(self).expect("PlanEntry holds only strings, integers and pairs, which always serialize")
    }
}

/// Why an untrusted plan entry (e.g. one the frontend sent back after the player edited a price)
/// was rejected. Port of the `ValueError`s `PlanEntry.from_dict` raises.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanEntryError {
    #[error("entry is missing unique_id")]
    MissingUniqueId,
    #[error("{field} must be a whole number")]
    NotAWholeNumber { field: &'static str },
    #[error("{field} is out of range")]
    OutOfRange { field: &'static str },
}

/// A JSON value that must be a whole number (not a bool, float, string or missing) within
/// `[minimum, maximum]`. Mirrors `market_lister._positive_int`; despite the name it allows 0 or
/// negative floors when the caller passes them (the range check is what actually enforces "positive").
fn positive_int(value: &Value, field: &'static str, minimum: i64, maximum: Option<i64>) -> Result<i64, PlanEntryError> {
    let Value::Number(n) = value else { return Err(PlanEntryError::NotAWholeNumber { field }) };
    let n = n.as_i64().ok_or(PlanEntryError::NotAWholeNumber { field })?;
    if n < minimum || maximum.is_some_and(|m| n > m) {
        return Err(PlanEntryError::OutOfRange { field });
    }
    Ok(n)
}

impl PlanEntry {
    /// Validates an untrusted plan entry (e.g. JSON from the frontend), the same bounds
    /// `PlanEntry.from_dict` enforces. `allow_unpriced` permits `price: 0` for an entry the game
    /// has not priced yet.
    pub fn from_dict(data: &Value, allow_unpriced: bool) -> Result<PlanEntry, PlanEntryError> {
        let obj = data.as_object();
        let unique_id = obj.and_then(|o| o.get("unique_id")).filter(|v| json_truthy(v)).map(json_str);
        let Some(unique_id) = unique_id.filter(|s| !s.trim().is_empty()) else {
            return Err(PlanEntryError::MissingUniqueId);
        };
        let get = |key: &str| obj.and_then(|o| o.get(key)).cloned().unwrap_or(Value::Null);
        let get_or = |key: &str, default: Value| obj.and_then(|o| o.get(key)).cloned().unwrap_or(default);

        let price_min = if allow_unpriced { 0 } else { 1 };
        let price = positive_int(&get("price"), "price", price_min, Some(MAX_LISTING_PRICE))?;
        let rarity = positive_int(&get_or("rarity", 0.into()), "rarity", 0, Some(8))?;
        let slot_id = positive_int(&get("slot_id"), "slot_id", 0, Some(239))?;
        let width = positive_int(&get_or("width", 1.into()), "width", 1, Some(4))?;
        let height = positive_int(&get_or("height", 1.into()), "height", 1, Some(MAX_ITEM_HEIGHT))?;
        let vendor_price = positive_int(&get_or("vendor_price", 0.into()), "vendor_price", 0, None)?;
        let quantity = positive_int(&get_or("quantity", 1.into()), "quantity", 1, Some(MAX_QUANTITY))?;
        let recommended = positive_int(&get_or("recommended", 0.into()), "recommended", 0, Some(MAX_LISTING_PRICE))?;
        let confidence = match get("confidence").as_str() {
            Some(s) if CONFIDENCE_LEVELS.contains(&s) => s.to_string(),
            _ => String::new(),
        };

        Ok(PlanEntry {
            unique_id,
            name: get_str_or(data, "name", "?", 128),
            rarity,
            stash_id: get_str_or(data, "stash_id", "", usize::MAX),
            slot_id,
            width,
            height,
            price,
            fee: if price != 0 { market::listing_fee(price) } else { 0 },
            vendor_price,
            item_id: get_str_or(data, "item_id", "", 128),
            base_rolls: stat_pairs(data.get("base_rolls")),
            rolls: stat_pairs(data.get("rolls")),
            flag: get_str_or(data, "flag", "", 300),
            compared: get_str_or(data, "compared", "", 300),
            confidence,
            quantity,
            recommended,
        })
    }
}

/// A finished plan: what will be listed, what was left out and why, and any hints for the player.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub entries: Vec<PlanEntry>,
    pub skipped: Vec<Skip>,
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn to_dict(&self) -> Value {
        let skipped: Vec<Value> = self
            .skipped
            .iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name, "stash_id": s.stash_id, "slot_id": s.slot_id, "reason": s.reason,
                    "flag": s.flag, "confidence": s.confidence, "unique_id": s.unique_id,
                    "merchant": market::is_merchant_reason(&s.reason),
                })
            })
            .collect();
        serde_json::json!({
            "entries": self.entries.iter().map(PlanEntry::to_dict).collect::<Vec<_>>(),
            "skipped": skipped,
            "warnings": self.warnings,
        })
    }
}

/// A plan entry built from a stash candidate; without a decision it is unpriced (price 0) until
/// the game or the in-game market prices it.
pub fn plan_entry(candidate: &Candidate, decision: Option<&market::PriceDecision>) -> PlanEntry {
    let item = &candidate.item;
    let price = decision.and_then(|d| d.price).unwrap_or(0);
    PlanEntry {
        unique_id: str_of(item, "itemUniqueId"),
        name: get_str(item, "name", "?", usize::MAX),
        rarity: market::rarity_id(item.get("rarity").unwrap_or(&Value::Null)),
        stash_id: candidate.stash_id.clone(),
        slot_id: get_i64(item, "slotId", 0),
        width: get_i64_or(item, "width", 1),
        height: get_i64_or(item, "height", 1),
        price,
        fee: decision.map_or(0, |d| d.fee),
        vendor_price: get_i64_or(item, "vendor_price", 0),
        item_id: get_str_or(item, "itemId", "", usize::MAX),
        base_rolls: stat_pairs(item.get("pp")),
        rolls: stat_pairs(item.get("sp")),
        flag: String::new(),
        compared: String::new(),
        confidence: String::new(),
        quantity: get_i64_or(item, "itemCount", 1).max(1),
        recommended: price,
    }
}

/// The marketplace tab icon index for `stash_id` under the player's tab mapping (inventory is
/// always icon 0), or `None` when the tab isn't mapped.
///
/// A pure copy of `marketplace_layout.tab_icon_index`: the rest of that module is screen-coordinate
/// math that belongs to the input crate, which already keeps its own independent copy of this same
/// check (`input::layout::marketplace::tab_icon_index`) rather than depend on a sibling crate for
/// one function — this follows the same precedent instead of adding an `input` dependency here.
fn tab_icon_index(stash_id: &str, tab_mapping: &[i64]) -> Option<usize> {
    if stash_id == market::INVENTORY_STASH_ID {
        return Some(0);
    }
    let stash_type: i64 = stash_id.parse().ok()?;
    if stash_type == 0 {
        return None;
    }
    tab_mapping.iter().position(|&v| v == stash_type).map(|i| i + 1)
}

/// A `Skip` for `candidate`, with an empty flag/confidence (used for structural reasons — an
/// unmapped tab, a stack needing game pricing — rather than a pricing doubt).
fn skip_candidate(candidate: &Candidate, reason: &str) -> Skip {
    Skip {
        name: get_str(&candidate.item, "name", "?", usize::MAX),
        stash_id: candidate.stash_id.clone(),
        slot_id: get_i64(&candidate.item, "slotId", 0),
        reason: reason.to_string(),
        flag: String::new(),
        confidence: String::new(),
        unique_id: get_str_or(&candidate.item, "itemUniqueId", "", usize::MAX),
    }
}

/// A `Skip` for a priced `entry` that turned out unlistable, optionally carrying a pricing doubt.
fn skip_entry(entry: &PlanEntry, reason: impl Into<String>, flag: &str, confidence: &str) -> Skip {
    Skip {
        name: entry.name.clone(),
        stash_id: entry.stash_id.clone(),
        slot_id: entry.slot_id,
        reason: reason.into(),
        flag: flag.to_string(),
        confidence: confidence.to_string(),
        unique_id: entry.unique_id.clone(),
    }
}

/// User-facing hints so an empty or thin plan never looks like nothing happened.
fn explain(entries: &[PlanEntry], skipped: &[Skip]) -> Vec<String> {
    let mut hints = Vec::new();
    if skipped.iter().any(|s| s.reason == UNMAPPED_TAB_REASON) {
        hints.push(
            "Some stash tabs aren't mapped — set them in Settings → Stash Tab Mapping \
             so the lister knows which icon opens which stash."
                .to_string(),
        );
    }
    if entries.is_empty() {
        hints.push(format!("No items to list — {} skipped. Open \"Skipped\" below to see why.", skipped.len()));
    }
    hints
}

/// Raised only for `error_code == "missing_api_key"`: every other DarkerDB error is recorded as a
/// warning instead, since a partial plan still has value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PlanError {
    pub code: String,
    pub message: String,
}

/// How many items `build_plan` may add this run: the rules' per-run cap, further limited by the
/// Marketplace's free listing spots (when known).
fn run_limit(rules: &ListerRules, free_spots: Option<i64>) -> i64 {
    match free_spots {
        None => rules.max_items_per_run,
        Some(spots) => rules.max_items_per_run.min(spots.max(0)),
    }
}

/// Drops candidates already up for sale (`exclude_unique_ids`), recording each as skipped rather
/// than silently vanishing — so a re-run after a partial list still explains every item.
fn drop_listed(candidates: Vec<Candidate>, skipped: &mut Vec<Skip>, exclude_unique_ids: &HashSet<String>) -> Vec<Candidate> {
    let mut kept = Vec::new();
    for candidate in candidates {
        if exclude_unique_ids.contains(&str_of(&candidate.item, "itemUniqueId")) {
            skipped.push(skip_candidate(&candidate, "already listed"));
        } else {
            kept.push(candidate);
        }
    }
    kept
}

/// Selects stash candidates, prices each (via `price_lookup`, or leaves them at price 0 for the
/// game to price when `price_lookup` is `None`), and explains everything left out.
///
/// `price_lookup(item)` returns a DarkerDB-shaped price check result (see `market::compute_price`);
/// `pause` is called once per lookup so a caller can throttle real network calls, and does nothing
/// by default. Stacks (`itemCount > 1`) are never auto-priced from DarkerDB, which quotes a single
/// unit: they are skipped with [`STACK_NEEDS_GAME_PRICING`] for the in-game market to price instead.
#[allow(clippy::too_many_arguments)]
pub fn build_plan(
    stashes: &HashMap<String, Vec<Value>>,
    rules: &ListerRules,
    price_lookup: Option<&PriceLookup<'_>>,
    tab_mapping: &[i64],
    free_spots: Option<i64>,
    data_age_s: Option<f64>,
    pause: &dyn Fn(),
    exclude_unique_ids: &HashSet<String>,
) -> Result<Plan, PlanError> {
    let (candidates, mut skipped) = market::select_candidates(stashes, rules);
    let candidates = drop_listed(candidates, &mut skipped, exclude_unique_ids);
    let candidate_count = candidates.len();
    let mut warnings = Vec::new();
    if data_age_s.is_some_and(|age| age > STALE_DATA_SECONDS) {
        let minutes = (data_age_s.expect("checked above") / 60.0).floor() as i64;
        warnings.push(format!("Stash data is {minutes} minutes old — reopen your character to refresh."));
    }

    let limit = run_limit(rules, free_spots);
    let mut entries: Vec<PlanEntry> = Vec::new();
    for candidate in &candidates {
        if entries.len() as i64 >= limit {
            break;
        }
        match price_one_candidate(candidate, rules, price_lookup, tab_mapping, pause)? {
            CandidateOutcome::Priced(entry) => entries.push(entry),
            CandidateOutcome::Skipped(skip) => skipped.push(skip),
            CandidateOutcome::RateLimited => {
                warnings.push("DarkerDB rate limit hit — plan is partial. Try again in a minute.".to_string());
                break;
            }
        }
    }

    if let Some(spots) = free_spots {
        if entries.len() as i64 >= spots && candidate_count > entries.len() {
            warnings.push(format!("Only {spots} free listing spots — some items were left out."));
        }
    }
    if price_lookup.is_none() && !entries.is_empty() {
        warnings.push("Prices will come from the in-game market — click \"Price from game\".".to_string());
    }
    warnings.extend(explain(&entries, &skipped));
    Ok(Plan { entries, skipped, warnings })
}

/// One candidate's fate: listable at a price, skipped with a reason, or the pricing run must stop
/// (DarkerDB is rate-limited).
enum CandidateOutcome {
    Priced(PlanEntry),
    Skipped(Skip),
    RateLimited,
}

/// Prices (or defers) a single candidate; factored out of `build_plan` so its loop body reads as a
/// short list of cases rather than one long nested function.
fn price_one_candidate(
    candidate: &Candidate,
    rules: &ListerRules,
    price_lookup: Option<&PriceLookup<'_>>,
    tab_mapping: &[i64],
    pause: &dyn Fn(),
) -> Result<CandidateOutcome, PlanError> {
    if tab_icon_index(&candidate.stash_id, tab_mapping).is_none() {
        return Ok(CandidateOutcome::Skipped(skip_candidate(candidate, UNMAPPED_TAB_REASON)));
    }
    let Some(price_lookup) = price_lookup else {
        return Ok(CandidateOutcome::Priced(plan_entry(candidate, None)));
    };
    if get_i64_or(&candidate.item, "itemCount", 1) > 1 {
        return Ok(CandidateOutcome::Skipped(skip_candidate(candidate, STACK_NEEDS_GAME_PRICING)));
    }
    let check = price_lookup(&candidate.item);
    pause();
    match check.get("error_code").and_then(Value::as_str) {
        Some("missing_api_key") => {
            return Err(PlanError {
                code: "missing_api_key".to_string(),
                message: "Add your DarkerDB API key (DARKERDB_API_KEY) to price items.".to_string(),
            })
        }
        Some("rate_limited") => return Ok(CandidateOutcome::RateLimited),
        _ => {}
    }
    let vendor_price = get_i64_or(&candidate.item, "vendor_price", 0);
    let decision = market::compute_price(Some(&check), vendor_price, rules);
    if decision.ok {
        Ok(CandidateOutcome::Priced(plan_entry(candidate, Some(&decision))))
    } else {
        Ok(CandidateOutcome::Skipped(skip_candidate(candidate, &decision.reason)))
    }
}

// --- pricing unpriced entries from the in-game market or the value formula ----------------------

/// One item's search results from the in-game Marketplace: listings of the exact same rolls, and
/// every listing of the item (any rolls), for `apply_game_prices` to compare against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarketBucket {
    pub same: Vec<MarketRow>,
    pub all: Vec<MarketRow>,
    /// A result page never arrived, so the search is incomplete (`"degraded"` in DnDTools'
    /// `price_all` results). The re-check before listing keeps the approved price when it is set.
    pub degraded: bool,
}

/// What `worth(entry)` may hand back to `apply_game_prices`: the full Item Worth estimate, or a
/// bare value with no floor. Mirrors `market_lister._model_prices`, which accepted either shape
/// from Python's dynamic typing; Rust needs the two cases spelled out.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelPrice {
    Value(f64),
    Estimate(market::Estimate),
}

/// `(model_value, model_floor)` keyword-equivalents for `price_from_market`.
fn model_prices(estimate: Option<ModelPrice>) -> (Option<f64>, Option<f64>) {
    match estimate {
        None => (None, None),
        Some(ModelPrice::Value(v)) => (Some(v), None),
        Some(ModelPrice::Estimate(e)) => (Some(e.value), Some(e.floor)),
    }
}

fn confidence_str(confidence: market::Confidence) -> &'static str {
    match confidence {
        market::Confidence::High => "high",
        market::Confidence::Medium => "medium",
        market::Confidence::Low => "low",
        market::Confidence::Unknown => "",
    }
}

/// Prices unpriced entries from in-game search results, comparing like rolls with like.
///
/// `market_by_unique_id` holds each entry's search results, keyed by `unique_id`; an entry missing
/// from it never got a search (the pricing run stopped first) and is skipped with
/// [`NOT_PRICED_REASON`]. `extra_rows(item_id)` widens the view with recent listings from the local
/// market history. `exclude_listing_ids` are our own listings, which must never set our own price.
/// `worth(entry)` is the Item Worth estimate for the entry's exact rolls: its value caps the price
/// and its floor sets the fast-sale price. `merchant_price(item_id)` is a hard ceiling: what a
/// merchant pays for one.
#[allow(clippy::too_many_arguments)]
pub fn apply_game_prices(
    entries: &[PlanEntry],
    market_by_unique_id: &HashMap<String, MarketBucket>,
    rules: &ListerRules,
    extra_rows: Option<&ExtraRows<'_>>,
    extra_share: f64,
    exclude_listing_ids: &HashSet<String>,
    synergies: Option<&HashMap<BTreeSet<String>, f64>>,
    worth: Option<&WorthLookup<'_>>,
    merchant_price: Option<&MerchantPriceLookup<'_>>,
) -> Plan {
    let mut priced = Vec::new();
    let mut skipped = Vec::new();
    for entry in entries {
        match price_from_game_market(entry, market_by_unique_id, rules, extra_rows, extra_share, exclude_listing_ids, synergies, worth, merchant_price) {
            Ok(priced_entry) => priced.push(priced_entry),
            Err(skip) => skipped.push(skip),
        }
    }
    let mut warnings = explain(&priced, &skipped);
    let flagged = priced.iter().filter(|e| !e.flag.is_empty()).count();
    if flagged > 0 {
        warnings.insert(0, format!("{flagged} price(s) marked \u{26a0}\u{fe0f} need your check before listing."));
    }
    Plan { entries: priced, skipped, warnings }
}

/// One entry's in-game pricing attempt: the priced copy, or why it was skipped.
///
/// Returns `Skip` (not a boxed/smaller error) on the error path: this runs at most
/// `rules.max_items_per_run` times per plan (well under 40), so the couple of extra stack bytes a
/// `market::Skip` costs over a boxed pointer here is immaterial.
#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn price_from_game_market(
    entry: &PlanEntry,
    market_by_unique_id: &HashMap<String, MarketBucket>,
    rules: &ListerRules,
    extra_rows: Option<&ExtraRows<'_>>,
    extra_share: f64,
    exclude_listing_ids: &HashSet<String>,
    synergies: Option<&HashMap<BTreeSet<String>, f64>>,
    worth: Option<&WorthLookup<'_>>,
    merchant_price: Option<&MerchantPriceLookup<'_>>,
) -> Result<PlanEntry, Skip> {
    let Some(bucket) = market_by_unique_id.get(&entry.unique_id) else {
        return Err(skip_entry(entry, NOT_PRICED_REASON, "", ""));
    };
    let not_excluded = |rows: &[MarketRow]| -> Vec<MarketRow> {
        rows.iter().filter(|r| !exclude_listing_ids.contains(&r.listing_id)).cloned().collect()
    };
    let same = not_excluded(&bucket.same);
    let mut all_and_history = bucket.all.clone();
    if let Some(extra_rows) = extra_rows {
        all_and_history.extend(extra_rows(&entry.item_id));
    }
    let all = not_excluded(&all_and_history);
    let (model_value, model_floor) = model_prices(worth.and_then(|f| f(entry)));
    let merchant_unit_price = merchant_price.and_then(|f| f(&entry.item_id));

    let result = market::price_from_market(
        &entry.item_id, &entry.base_rolls, &entry.rolls, entry.vendor_price, &same, &all, rules, extra_share,
        entry.quantity, synergies, model_value, model_floor, merchant_unit_price,
    );
    if result.ok && result.price.is_some_and(|p| p > MAX_LISTING_PRICE) {
        return Err(skip_entry(entry, ABOVE_MAX_REASON, &result.flag, &result.confidence));
    }
    if !result.ok {
        return Err(skip_entry(entry, result.reason, &result.flag, &result.confidence));
    }
    let price = result.price.expect("price_from_market sets a price whenever it reports ok");
    Ok(PlanEntry {
        price,
        fee: result.fee,
        flag: result.flag,
        compared: result.compared,
        confidence: result.confidence,
        recommended: price,
        ..entry.clone()
    })
}

/// Prices entries at the value formula's lowest reasonable price — no market lookups at all.
pub fn price_from_model(entries: &[PlanEntry], rules: &ListerRules, worth: Option<&ModelWorthLookup<'_>>) -> Plan {
    let mut priced = Vec::new();
    let mut skipped = Vec::new();
    for entry in entries {
        match price_one_from_model(entry, rules, worth) {
            Ok(priced_entry) => priced.push(priced_entry),
            Err(skip) => skipped.push(skip),
        }
    }
    let warnings = explain(&priced, &skipped);
    Plan { entries: priced, skipped, warnings }
}

/// Same `Skip`-over-`Box<Skip>` reasoning as [`price_from_game_market`]'s doc comment.
#[allow(clippy::result_large_err)]
fn price_one_from_model(
    entry: &PlanEntry,
    rules: &ListerRules,
    worth: Option<&ModelWorthLookup<'_>>,
) -> Result<PlanEntry, Skip> {
    let estimate = worth.and_then(|f| f(entry));
    let floor = estimate.as_ref().map(|e| e.floor).filter(|f| *f != 0.0);
    let Some(floor) = floor else {
        return Err(skip_entry(entry, MODEL_NO_DATA_REASON, "", ""));
    };
    let price = floor.floor() as i64;
    let fee = market::listing_fee(price);
    let reason = if price > MAX_LISTING_PRICE {
        Some(ABOVE_MAX_REASON)
    } else if price < rules.min_price.max(1) {
        Some("below min price")
    } else if price - fee <= entry.vendor_price * entry.quantity {
        Some("vendor pays more")
    } else if ((price - fee) as f64 / price as f64) < rules.min_net_ratio {
        Some("fee too high")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(skip_entry(entry, reason, "", ""));
    }
    let estimate = estimate.expect("floor is only Some when estimate is Some");
    let compared = format!(
        "value formula: lowest reasonable price for these rolls {price}g (typical ask {}g)",
        market::card::round_half_even(estimate.value)
    );
    Ok(PlanEntry { price, fee, recommended: price, confidence: confidence_str(estimate.confidence).to_string(), compared, ..entry.clone() })
}
