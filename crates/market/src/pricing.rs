//! Roll-aware pricing from in-game Marketplace search results (pure, no I/O).
//!
//! Port of DnDTools' `src/models/roll_pricing.py`.
//!
//! How a price is built (low, for a fast sale - never a point inside a price range):
//!
//! 1. Anchors. For each of our random rolls, the anchor is one real listing: the cheapest
//!    non-lowball copy with that stat at our level or the nearest similar weaker level (within
//!    [`CLOSE_ROLL_RATIO`]) - or anything stronger that happens to be cheaper. So we are never
//!    dearer than a copy at least as good, nor than a slightly weaker one. Rolls far weaker or
//!    far stronger than ours are no comparison: a small roll isn't priced like a god roll and one
//!    expensive listing can't drag the price up.
//! 2. Best roll + extra good rolls. The best roll's anchor is the base. Every other roll that
//!    commands a premium over a plain copy adds a share of that premium (a learned pair bonus
//!    when the two stats sell together, else [`EXTRA_ROLL_SHARE`]).
//! 3. Ceiling. A listing with the same rolls that is at least as good on every stat caps the
//!    price: nobody pays more for ours than for a better copy.
//! 4. Items without rolls use the cheapest real (non-lowball) listing.
//!
//! Lowballs (asks far below comparable copies) are ignored everywhere - they rarely last.
//!
//! Base stats count toward "at least as good" with a small tolerance, since they vary a little
//! between otherwise identical copies. Every price carries a confidence level and a plain
//! explanation for the review table.

use std::collections::{BTreeSet, HashMap};

use crate::card::round_half_even;
use crate::history::Listing;
use crate::rules::{listing_fee, ListerRules};

/// `DesignDataItemPropertyType:Id_ItemPropertyType_Effect_Luck` -> `Effect_`'s prefix.
pub const PROPERTY_PREFIX: &str = "Effect_";
pub const NO_SELLERS_REASON: &str = "nobody is selling this right now";
/// An ask under half the typical ask of comparable copies is a lowball.
pub const LOWBALL_RATIO: f64 = 0.5;
/// Rolls within 1.5x of ours (either way) are similar enough to compare.
pub const CLOSE_ROLL_RATIO: f64 = 1.5;
/// Base stats within 5% (at least 1 point) count as equal.
pub const BASE_TOLERANCE: f64 = 0.05;
/// Fallback share of an extra roll's premium when no pair synergy is known.
pub const EXTRA_ROLL_SHARE: f64 = 0.25;
/// Cap on a learned pair bonus.
pub const MAX_SYNERGY_PCT: f64 = 50.0;
pub const FAR_APART_FLAG: &str = "Only two listings and they're far apart — check this price.";

/// One listing read from a Marketplace search: an item, its price, and its base/roll stats.
///
/// `base`/`rolls` are `(stat, value)` pairs, matching the game-data unit convention (percent
/// stats ×10) used by [`crate::history::Stat`].
#[derive(Debug, Clone, PartialEq)]
pub struct MarketRow {
    pub item_id: String,
    /// Price of the whole listing (all `count` items), same convention as [`Listing::price`].
    pub price: i64,
    pub base: Vec<(String, i64)>,
    pub rolls: Vec<(String, i64)>,
    pub listing_id: String,
    /// Stack size of the listing; its price is for the whole stack.
    pub count: i64,
}

impl From<&Listing> for MarketRow {
    fn from(listing: &Listing) -> Self {
        MarketRow {
            item_id: listing.item_id.clone(),
            price: listing.price,
            base: listing.base.iter().map(|s| (s.id.clone(), s.value)).collect(),
            rolls: listing.rolls.iter().map(|s| (s.id.clone(), s.value)).collect(),
            listing_id: listing.listing_id.clone(),
            count: listing.count,
        }
    }
}

/// The result of pricing one item (or stack) from the market.
#[derive(Debug, Clone, PartialEq)]
pub struct RollPrice {
    pub ok: bool,
    pub price: Option<i64>,
    pub fee: i64,
    pub reason: String,
    /// A doubt about the price that a reviewer should check.
    pub flag: String,
    /// A plain explanation of how the price was reached, for the review table.
    pub compared: String,
    /// "high" | "medium" | "low" | "" (no comparable listing was found).
    pub confidence: String,
}

/// What a buyer who wants a given roll value pays elsewhere: one real listing's price, how that
/// was decided, and how sure we are of it.
#[derive(Debug, Clone, PartialEq)]
struct Anchor {
    value: f64,
    how: String,
    confidence: &'static str,
    /// Whether this roll beats every listing found (no real comparison exists).
    beats_all: bool,
}

/// `DesignDataItemPropertyType:Id_ItemPropertyType_Effect_Luck` -> `Luck`.
pub fn stat_name(property_type_id: &str) -> &str {
    property_type_id.rsplit(PROPERTY_PREFIX).next().unwrap_or(property_type_id)
}

/// Last-value-wins lookup over `(stat, value)` pairs, matching Python's `dict(pairs)`.
fn as_map(pairs: &[(String, i64)]) -> HashMap<&str, i64> {
    pairs.iter().map(|(k, v)| (k.as_str(), *v)).collect()
}

/// Whether `row`'s base stats are at least as good as ours, within a small tolerance (base stats
/// vary a little between otherwise identical copies).
fn base_ok(row: &MarketRow, base: &[(String, i64)]) -> bool {
    let theirs = as_map(&row.base);
    base.iter().all(|(stat, value)| {
        let value = *value as f64;
        let theirs_value = theirs.get(stat.as_str()).map_or(f64::NEG_INFINITY, |v| *v as f64);
        theirs_value >= value - 1.0_f64.max(value.abs() * BASE_TOLERANCE)
    })
}

/// Whether `row` is at least as good as us on every base stat and every one of `rolls`.
fn dominates(row: &MarketRow, base: &[(String, i64)], rolls: &[(String, i64)]) -> bool {
    let theirs = as_map(&row.rolls);
    base_ok(row, base) && rolls.iter().all(|(stat, value)| theirs.get(stat.as_str()).is_some_and(|v| *v >= *value))
}

/// The lower of the two middle values (the exact middle for an odd count), matching Python's
/// `statistics.median_low`. Every call site passes a non-empty slice.
fn median_low(values: &[f64]) -> f64 {
    assert!(!values.is_empty(), "median_low requires at least one value");
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        sorted[n / 2 - 1]
    }
}

/// Cheapest price, ignoring lowball listings far below the typical (median) ask.
///
/// The lower median is used so a single wild high ask can't drag the threshold up and throw away
/// every normal listing. Every call site passes a non-empty slice.
fn sane_min(prices: &[f64]) -> f64 {
    let typical = median_low(prices);
    let filtered: Vec<f64> = prices.iter().copied().filter(|&p| p >= LOWBALL_RATIO * typical).collect();
    let candidates: &[f64] = if filtered.is_empty() { prices } else { &filtered };
    candidates.iter().copied().fold(f64::INFINITY, f64::min)
}

/// Drops lowball asks from `(roll value, price)` points.
///
/// A listing is judged only against copies no better than it: a cheap weak roll is a real price
/// level, while a strong roll dumped far below weaker copies is a lowball.
fn without_lowballs(points: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let kept: Vec<(i64, i64)> = points
        .iter()
        .copied()
        .filter(|&(v, p)| {
            let comparable: Vec<f64> = points.iter().filter(|&&(w, _)| w <= v).map(|&(_, q)| q as f64).collect();
            p as f64 >= LOWBALL_RATIO * median_low(&comparable)
        })
        .collect();
    if kept.is_empty() {
        points.to_vec()
    } else {
        kept
    }
}

/// Both searches can return the same listing; keep each once (last one wins on a duplicate key,
/// matching Python's dict-comprehension dedup).
fn unique(rows: impl IntoIterator<Item = MarketRow>) -> Vec<MarketRow> {
    #[derive(Clone, PartialEq, Eq, Hash)]
    enum Key {
        ListingId(String),
        Fields(String, i64, Vec<(String, i64)>, Vec<(String, i64)>),
    }
    let mut order: Vec<Key> = Vec::new();
    let mut latest: HashMap<Key, MarketRow> = HashMap::new();
    for row in rows {
        let key = if row.listing_id.is_empty() {
            Key::Fields(row.item_id.clone(), row.price, row.base.clone(), row.rolls.clone())
        } else {
            Key::ListingId(row.listing_id.clone())
        };
        if !latest.contains_key(&key) {
            order.push(key.clone());
        }
        latest.insert(key, row);
    }
    order.into_iter().map(|k| latest.remove(&k).expect("key was just inserted")).collect()
}

fn undercut(reference: f64, rules: &ListerRules) -> i64 {
    (reference * (1.0 - rules.undercut_pct / 100.0)).floor() as i64
}

fn roll_value(row: &MarketRow, stat: &str) -> Option<i64> {
    as_map(&row.rolls).get(stat).copied()
}

fn confidence_rank(confidence: &str) -> u8 {
    match confidence {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 3,
    }
}

/// What a buyer who wants our `value` of `stat` pays elsewhere: one real listing's price.
fn anchor_estimate(rows: &[MarketRow], base: &[(String, i64)], stat: &str, value: i64) -> Option<Anchor> {
    let points: Vec<(i64, i64)> =
        rows.iter().filter(|r| base_ok(r, base)).filter_map(|r| roll_value(r, stat).map(|v| (v, r.price))).collect();
    let mut real = without_lowballs(&points);
    if real.is_empty() {
        return None;
    }
    if real.iter().all(|&(v, _)| v < value) {
        let best_v = real.iter().map(|&(v, _)| v).max().expect("real is non-empty");
        let best_p = real.iter().filter(|&&(v, _)| v == best_v).map(|&(_, p)| p).min().expect("real is non-empty");
        return Some(Anchor {
            value: best_p as f64,
            how: format!("{stat} {value} beats every listing (best {stat} {best_v} asks {best_p}g)"),
            confidence: "low",
            beats_all: true,
        });
    }
    let (low, high) = if value > 0 {
        (value as f64 / CLOSE_ROLL_RATIO, value as f64 * CLOSE_ROLL_RATIO)
    } else {
        (value as f64, value as f64)
    };
    let similar_better: Vec<i64> =
        real.iter().filter(|&&(v, _)| value as f64 <= v as f64 && v as f64 <= high).map(|&(_, p)| p).collect();
    if !similar_better.is_empty() {
        // A weaker copy far below similar ones at least as good as ours is a dump.
        let floor = LOWBALL_RATIO * similar_better.iter().copied().min().expect("non-empty") as f64;
        real.retain(|&(v, p)| v as f64 >= value as f64 || p as f64 >= floor);
    }
    // The nearest similar weaker level.
    let level = real.iter().filter(|&&(v, _)| low <= v as f64 && (v as f64) < value as f64).map(|&(v, _)| v).max();
    let level = level.unwrap_or(value);
    let competing: Vec<(i64, i64)> = real.iter().copied().filter(|&(v, _)| v as f64 >= level as f64).collect();
    if competing.iter().all(|&(v, _)| v as f64 > high) {
        return None; // Only much stronger rolls are listed: this roll doesn't set our price.
    }
    let price = competing.iter().map(|&(_, p)| p).min().expect("competing is non-empty (checked above)");
    let confidence =
        if !similar_better.is_empty() && real.iter().any(|&(v, _)| (v as f64) < value as f64) { "high" } else { "medium" };
    Some(Anchor {
        value: price as f64,
        how: format!("{stat} {value}: cheapest listing with {stat} \u{2265} {level} asks {price}g"),
        confidence,
        beats_all: false,
    })
}

/// `(gold, explanation)` added for rolls beyond the best one.
///
/// Market data shows unrelated extra rolls add little, but rolls that suit the same build sell
/// for more together: a learned pair synergy (percent of the best roll's price) wins, otherwise a
/// small share of the extra roll's own premium is added.
fn extra_roll_bonus(
    best_stat: &str,
    best_value: f64,
    extras: &[(String, i64, f64)],
    extra_share: f64,
    synergies: Option<&HashMap<BTreeSet<String>, f64>>,
) -> (f64, String) {
    let mut bonus = 0.0;
    let mut parts: Vec<String> = Vec::new();
    for (stat, value, premium) in extras {
        let pair = BTreeSet::from([best_stat.to_string(), stat.clone()]);
        let synergy_pct = synergies.and_then(|m| m.get(&pair).copied()).filter(|p| *p > 0.0);
        let gain = match synergy_pct {
            Some(pct) => {
                parts.push(format!("{stat} pairs with {best_stat} (+{pct:.0}% in market data)"));
                best_value * pct.min(MAX_SYNERGY_PCT) / 100.0
            }
            None => {
                let gain = premium.max(0.0) * extra_share;
                if gain != 0.0 {
                    parts.push(format!("{stat} {value}"));
                }
                gain
            }
        };
        bonus += gain;
    }
    let how = if bonus != 0.0 && !parts.is_empty() {
        format!("; +{}g for extra rolls ({})", round_half_even(bonus), parts.join(", "))
    } else {
        String::new()
    };
    (bonus, how)
}

/// Best roll's anchor price plus a bonus for the other rolls.
fn combine(
    rows: &[MarketRow],
    base: &[(String, i64)],
    rolls: &[(String, i64)],
    baseline: i64,
    extra_share: f64,
    synergies: Option<&HashMap<BTreeSet<String>, f64>>,
) -> Option<(f64, String, &'static str, bool)> {
    let mut estimates: Vec<(&str, i64, Anchor)> = rolls
        .iter()
        .filter_map(|(stat, value)| anchor_estimate(rows, base, stat, *value).map(|est| (stat.as_str(), *value, est)))
        .collect();
    if estimates.is_empty() {
        return None;
    }
    // Ties: the surer one (Vec::sort_by is stable, matching Python's list.sort).
    estimates.sort_by(|a, b| {
        b.2.value.total_cmp(&a.2.value).then_with(|| confidence_rank(a.2.confidence).cmp(&confidence_rank(b.2.confidence)))
    });
    let (best_stat, _, best) = estimates[0].clone();
    let extras: Vec<(String, i64, f64)> =
        estimates[1..].iter().map(|(s, v, e)| ((*s).to_string(), *v, e.value - baseline as f64)).collect();
    let (bonus, extra_how) = extra_roll_bonus(best_stat, best.value, &extras, extra_share, synergies);
    let beats_any = estimates.iter().any(|(_, _, e)| e.beats_all);
    Some((best.value + bonus, format!("{}{extra_how}", best.how), best.confidence, beats_any))
}

/// Stacks sell for less per unit than single items: compare a stack with other stacks and a
/// single item with other singles whenever the market has any.
fn like_for_like(rows: &[MarketRow], quantity: i64) -> Vec<MarketRow> {
    let bulk: Vec<MarketRow> = rows.iter().filter(|r| r.count > 1).cloned().collect();
    let singles: Vec<MarketRow> = rows.iter().filter(|r| r.count <= 1).cloned().collect();
    if quantity > 1 && !bulk.is_empty() {
        bulk
    } else if quantity <= 1 && !singles.is_empty() {
        singles
    } else {
        rows.to_vec()
    }
}

/// `(reference, flag, explanation, confidence, floor)` for items without rolls.
fn stack_reference(rows: &[MarketRow], quantity: i64) -> (f64, String, String, &'static str, f64) {
    let rows = like_for_like(rows, quantity);
    let units: Vec<f64> = rows.iter().map(|r| r.price as f64 / r.count.max(1) as f64).collect();
    let unit = sane_min(&units);
    let mut confidence: &'static str = if rows.len() >= 3 { "high" } else { "medium" };
    let mut flag = String::new();
    if units.len() == 2 {
        // Can't tell which one is off.
        let lo = units.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = units.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if lo < LOWBALL_RATIO * hi {
            confidence = "low";
            flag = FAR_APART_FLAG.to_string();
        }
    }
    if quantity == 1 && rows.iter().all(|r| r.count == 1) {
        let rounded = round_half_even(unit);
        let how = format!("cheapest of {} listings: {rounded}g", rows.len());
        return (rounded as f64, flag, how, confidence, rounded as f64);
    }
    let how = format!("cheapest of {} listings: {unit:.1}g per unit x {quantity}", rows.len());
    (unit * quantity as f64, flag, how, confidence, unit * quantity as f64)
}

/// `(reference price, flag, explanation, confidence, floor)` or `None` when nobody sells this
/// item. `floor` is the cheapest real listing of the item, whatever its rolls.
#[allow(clippy::too_many_arguments)] // faithful port of Python's `_reference`, which takes these as named parameters
fn reference(
    item_id: &str,
    base: &[(String, i64)],
    rolls: &[(String, i64)],
    same_rows: &[MarketRow],
    all_rows: &[MarketRow],
    // Unused here, same as Python's `_reference`: kept so the signature mirrors `price_from_market`.
    _rules: &ListerRules,
    extra_share: f64,
    quantity: i64,
    synergies: Option<&HashMap<BTreeSet<String>, f64>>,
) -> Option<(f64, String, String, &'static str, f64)> {
    let rows = unique(same_rows.iter().chain(all_rows.iter()).filter(|r| r.item_id == item_id).cloned());
    if rows.is_empty() {
        return None;
    }
    if rolls.is_empty() {
        let (value, flag, how, confidence, floor) = stack_reference(&rows, quantity);
        return Some((value, flag, how, confidence, floor));
    }
    let prices: Vec<f64> = rows.iter().map(|r| r.price as f64).collect();
    let baseline = sane_min(&prices).round() as i64;
    let combined = combine(&rows, base, rolls, baseline, extra_share, synergies);
    let roll_set: BTreeSet<&str> = rolls.iter().map(|(s, _)| s.as_str()).collect();
    let dominating: Vec<i64> = rows
        .iter()
        .filter(|r| {
            let row_roll_set: BTreeSet<&str> = r.rolls.iter().map(|(s, _)| s.as_str()).collect();
            row_roll_set == roll_set && dominates(r, base, rolls)
        })
        .map(|r| r.price)
        .collect();
    let (mut value, mut how, confidence, beats_all) = match combined {
        Some(c) => c,
        None => {
            let flag =
                "There are no listings with rolls like yours — priced against copies of any roll; check this price."
                    .to_string();
            let how = format!("{} listings of any roll; cheapest {baseline}g", rows.len());
            return Some((baseline as f64, flag, how, "low", baseline as f64));
        }
    };
    let dearest = rows.iter().map(|r| r.price).max().expect("rows is non-empty (checked above)");
    if value > dearest as f64 {
        // Never above the most expensive listing of this item.
        value = dearest as f64;
        how = format!("{how}; capped at the dearest listing ({dearest}g)");
    }
    // A dumped better copy is no ceiling.
    let real_caps: Vec<i64> = dominating.iter().copied().filter(|&p| p as f64 >= LOWBALL_RATIO * value).collect();
    if !real_caps.is_empty() {
        let ceiling = real_caps.iter().copied().min().expect("non-empty");
        if value > ceiling as f64 {
            value = ceiling as f64;
            how = format!("{how}; capped at {ceiling}g (a copy at least as good on every stat)");
        }
    }
    let mut flag = String::new();
    if beats_all && dominating.is_empty() {
        flag = "Your rolls beat everything listed — consider pricing higher yourself.".to_string();
    } else if confidence == "low" {
        flag = "Few comparable listings — check this price.".to_string();
    }
    Some((value, flag, how, confidence, baseline as f64))
}

/// Prices our copy (or our stack of `quantity`); `vendor_price` is per unit.
///
/// `synergies`: learned pair bonuses (percent), keyed by the two stat names. `model_value`: the
/// Item Worth model's value for these exact rolls (whole quantity). The reference never exceeds
/// it - a price inherited from a listing's *other* stats can't stick to junk rolls - and never
/// drops below the cheapest real listing of the item. `model_floor`: the model's lowest reasonable
/// price for these rolls (whole quantity). For a fast sale we list at the lower of it and the
/// usual undercut, but never below half of it (dumps). `merchant_unit_price`: what a merchant
/// sells one for - nobody pays more on the market.
#[allow(clippy::too_many_arguments)] // faithful port of Python's `price_from_market`, which takes these as named/keyword parameters
pub fn price_from_market(
    item_id: &str,
    base: &[(String, i64)],
    rolls: &[(String, i64)],
    vendor_price: i64,
    same_rows: &[MarketRow],
    all_rows: &[MarketRow],
    rules: &ListerRules,
    extra_share: f64,
    quantity: i64,
    synergies: Option<&HashMap<BTreeSet<String>, f64>>,
    model_value: Option<f64>,
    model_floor: Option<f64>,
    merchant_unit_price: Option<f64>,
) -> RollPrice {
    let found = reference(item_id, base, rolls, same_rows, all_rows, rules, extra_share, quantity, synergies);
    let (mut ref_price, mut flag, mut compared, confidence, floor) = match found {
        Some(f) => f,
        None => {
            return RollPrice {
                ok: false,
                price: None,
                fee: 0,
                reason: NO_SELLERS_REASON.to_string(),
                flag: String::new(),
                compared: String::new(),
                confidence: String::new(),
            }
        }
    };

    if let Some(model_value) = model_value.filter(|v| *v != 0.0 && *v < ref_price) {
        let capped = model_value.max(floor);
        if capped < ref_price {
            ref_price = capped;
            compared =
                format!("{compared}; capped at the value model's {}g for these exact rolls", round_half_even(model_value));
            // Beating every listing on a stat buyers don't value is no reason to price higher.
            if flag.starts_with("Your rolls beat") {
                flag.clear();
            }
        }
    }

    let shop = merchant_unit_price.filter(|p| *p != 0.0).map(|p| p * quantity as f64);
    if let Some(shop) = shop {
        if shop < ref_price {
            ref_price = shop;
            compared = format!("{compared}; capped at the merchant's shop price ({}g)", round_half_even(shop));
        }
    }

    let mut price = undercut(ref_price, rules);
    if let Some(model_floor) = model_floor.filter(|f| *f != 0.0) {
        let floor_applies = match shop {
            None => true,
            Some(s) => model_floor <= s,
        };
        if floor_applies {
            let floor_i = model_floor.floor() as i64;
            let half_floor_i = (LOWBALL_RATIO * model_floor).floor() as i64;
            let fast = price.min(floor_i).max(half_floor_i);
            if fast != price {
                compared = if fast == floor_i {
                    format!("{compared}; fast sale at the lowest reasonable price for these rolls ({floor_i}g)")
                } else {
                    format!("{compared}; not below half the lowest reasonable price ({floor_i}g)")
                };
            }
            price = fast;
        }
    }

    if price < rules.min_price.max(1) {
        let reason = match shop {
            Some(s) if s == ref_price => format!("a merchant sells it for {}g", round_half_even(s)),
            _ => "below min price".to_string(),
        };
        return RollPrice { ok: false, price: None, fee: 0, reason, flag, compared, confidence: confidence.to_string() };
    }
    let fee = listing_fee(price);
    let net = price - fee;
    if net <= vendor_price * quantity {
        return RollPrice {
            ok: false,
            price: None,
            fee: 0,
            reason: "vendor pays more".to_string(),
            flag,
            compared,
            confidence: confidence.to_string(),
        };
    }
    if (net as f64 / price as f64) < rules.min_net_ratio {
        return RollPrice {
            ok: false,
            price: None,
            fee: 0,
            reason: "fee too high".to_string(),
            flag,
            compared,
            confidence: confidence.to_string(),
        };
    }
    RollPrice { ok: true, price: Some(price), fee, reason: "ok".to_string(), flag, compared, confidence: confidence.to_string() }
}
