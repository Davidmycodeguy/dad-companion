//! Cross-item pattern analysis over the local market history (pure, no I/O): port of
//! `market_patterns.py` and `market_model.py`.
//!
//! Prices are compared *within* an item: each listing's value is its log price relative to the
//! median log price of the same item and rarity. That cancels out how expensive the item is in
//! general and leaves what its rolls add. From that we learn roll ranges, how much each stat adds,
//! stat pairs worth more together, and market habits (rarity steps, price points, lowballs).
//!
//! `analyze` produces the full report; [`model_from_report`] keeps the subset pricing needs
//! ([`pair_bonuses`], [`extra_roll_share`]), saved as `market_model.json`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::time::Duration;

const GOOD_ROLL_PERCENTILE: f64 = 0.7;
const MIN_ITEM_LISTINGS: usize = 8;
const MODEL_PAIRS: usize = 40;
const MIN_STAT_SUPPORT: usize = 8;
const LOWBALL_RATIO: f64 = 0.5;

/// One recorded listing, as pattern analysis needs it: port of `market_patterns.py`'s `Listing`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Listing {
    pub item_id: String,
    pub rarity: i64,
    pub price: i64,
    pub item_count: i64,
    pub base: Vec<(String, f64)>,
    pub rolls: Vec<(String, f64)>,
    pub seller: String,
}

/// Single-item listings of items with at least [`MIN_ITEM_LISTINGS`] of them (a group too thin to
/// compare within itself).
fn group_by_item(listings: &[Listing]) -> HashMap<&str, Vec<&Listing>> {
    let mut groups: HashMap<&str, Vec<&Listing>> = HashMap::new();
    for listing in listings {
        if listing.item_count == 1 && listing.price > 0 {
            groups.entry(listing.item_id.as_str()).or_default().push(listing);
        }
    }
    groups.retain(|_, rows| rows.len() >= MIN_ITEM_LISTINGS);
    groups
}

/// `{item_id: {stat: (min, max, count)}}` over random rolls.
pub fn roll_ranges(listings: &[Listing]) -> BTreeMap<String, BTreeMap<String, (f64, f64, usize)>> {
    let mut values: HashMap<&str, HashMap<&str, Vec<f64>>> = HashMap::new();
    for listing in listings {
        for (stat, value) in &listing.rolls {
            values.entry(listing.item_id.as_str()).or_default().entry(stat.as_str()).or_default().push(*value);
        }
    }
    values
        .into_iter()
        .map(|(item, stats)| {
            let spans = stats
                .into_iter()
                .map(|(stat, v)| {
                    let lo = v.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    (stat.to_owned(), (lo, hi, v.len()))
                })
                .collect();
            (item.to_owned(), spans)
        })
        .collect()
}

/// Where `value` sits between `low` and `high`, 0..1; 1.0 when the range is degenerate (note this
/// differs from `crate::worth::roll_quality`, which defaults to 0.5 — the two are separate
/// functions in the Python source with separate defaults).
pub fn percentile(value: f64, low: f64, high: f64) -> f64 {
    if high <= low {
        1.0
    } else {
        ((value - low) / (high - low)).clamp(0.0, 1.0)
    }
}

/// The middle value (the mean of the two middle ones for an even count).
fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// `(listing, log price minus the median log price of the *other* listings of that item)`.
/// Leaving the listing itself out keeps small groups from pinning results to exactly 0.
fn relative_prices<'a>(groups: &HashMap<&'a str, Vec<&'a Listing>>) -> Vec<(&'a Listing, f64)> {
    let mut out = Vec::new();
    for rows in groups.values() {
        let logs: Vec<f64> = rows.iter().map(|r| (r.price as f64).ln()).collect();
        for (i, (&row, &lp)) in rows.iter().zip(&logs).enumerate() {
            let others: Vec<f64> = logs.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, &v)| v).collect();
            out.push((row, lp - median(&others)));
        }
    }
    out
}

/// A stat's price premium, in percent: port of `market_patterns.py`'s `stat_premiums` dict entry.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct StatPremium {
    /// Median price uplift of listings carrying the stat vs. the item's median.
    pub present: f64,
    /// Extra uplift going from the worst to the best observed roll of it.
    pub per_quality: f64,
    pub support: usize,
}

/// Each stat's price premium, most valuable first (the order `worth_model.py`'s tests rely on;
/// callers writing this to JSON should collect it into a map, where order is irrelevant).
pub fn stat_premiums(listings: &[Listing]) -> Vec<(String, StatPremium)> {
    let groups = group_by_item(listings);
    let all_rows: Vec<&Listing> = groups.values().flatten().copied().collect();
    let ranges = roll_ranges(&all_rows.iter().map(|r| (*r).clone()).collect::<Vec<_>>());
    let mut by_stat: HashMap<&str, Vec<(f64, f64)>> = HashMap::new();
    for (listing, rel) in relative_prices(&groups) {
        for (stat, value) in &listing.rolls {
            let (lo, hi, _) = ranges[&listing.item_id][stat];
            by_stat.entry(stat.as_str()).or_default().push((rel, percentile(*value, lo, hi)));
        }
    }
    let mut result: Vec<(String, StatPremium)> = by_stat
        .into_iter()
        .filter(|(_, points)| points.len() >= MIN_STAT_SUPPORT)
        .map(|(stat, points)| {
            let rels: Vec<f64> = points.iter().map(|(r, _)| *r).collect();
            let quals: Vec<f64> = points.iter().map(|(_, q)| *q).collect();
            let (mq, mr) = (mean(&quals), mean(&rels));
            let var: f64 = quals.iter().map(|q| (q - mq).powi(2)).sum();
            let slope = if var > 1e-9 { points.iter().map(|(r, q)| (q - mq) * (r - mr)).sum::<f64>() / var } else { 0.0 };
            let premium =
                StatPremium { present: round1((median(&rels).exp() - 1.0) * 100.0), per_quality: round1((slope.exp() - 1.0) * 100.0), support: points.len() };
            (stat.to_owned(), premium)
        })
        .collect();
    result.sort_by(|a, b| b.1.per_quality.total_cmp(&a.1.per_quality));
    result
}

/// A good-roll-count bucket's uplift: port of `market_patterns.py`'s `good_roll_counts` dict entry.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct GoodRollCount {
    pub median_uplift: f64,
    pub support: usize,
}

/// `{k: uplift}` for listings with `k` rolls in the top 30% of their observed range.
pub fn good_roll_counts(listings: &[Listing]) -> BTreeMap<u32, GoodRollCount> {
    let groups = group_by_item(listings);
    let all_rows: Vec<Listing> = groups.values().flatten().map(|r| (*r).clone()).collect();
    let ranges = roll_ranges(&all_rows);
    let mut by_k: HashMap<u32, Vec<f64>> = HashMap::new();
    for (listing, rel) in relative_prices(&groups) {
        let mut good = 0u32;
        for (stat, value) in &listing.rolls {
            let (lo, hi, count) = ranges[&listing.item_id][stat];
            if count >= 3 && percentile(*value, lo, hi) >= GOOD_ROLL_PERCENTILE {
                good += 1;
            }
        }
        by_k.entry(good).or_default().push(rel);
    }
    by_k.into_iter().map(|(k, v)| (k, GoodRollCount { median_uplift: round1((median(&v).exp() - 1.0) * 100.0), support: v.len() })).collect()
}

/// How much of the best good roll's uplift each further good roll adds (0..1), or `None` when
/// there isn't enough support to tell.
pub fn extra_good_roll_factor(counts: &BTreeMap<u32, GoodRollCount>) -> Option<f64> {
    let one = counts.get(&1)?;
    let two = counts.get(&2)?;
    if one.support.min(two.support) < MIN_STAT_SUPPORT {
        return None;
    }
    let zero_uplift = counts.get(&0).map_or(0.0, |c| c.median_uplift);
    let first = (one.median_uplift / 100.0).ln_1p() - (zero_uplift / 100.0).ln_1p();
    let second = (two.median_uplift / 100.0).ln_1p() - (one.median_uplift / 100.0).ln_1p();
    if first <= 0.0 {
        return None;
    }
    Some(round2((second / first).clamp(0.0, 1.0)))
}

/// A stat pair's synergy: port of `market_patterns.py`'s `pair_synergies` list entry.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PairSynergy {
    pub pair: String,
    /// Percent premium of listings with both stats over their best separate uplift.
    pub synergy: f64,
    pub support: usize,
}

/// Stat pairs whose listings are worth more than their stats' separate uplifts suggest, best
/// first, at most `top`.
pub fn pair_synergies(listings: &[Listing], top: usize) -> Vec<PairSynergy> {
    let groups = group_by_item(listings);
    let rel_rows = relative_prices(&groups);
    let mut single: HashMap<&str, Vec<f64>> = HashMap::new();
    for (listing, rel) in &rel_rows {
        for (stat, _) in &listing.rolls {
            single.entry(stat.as_str()).or_default().push(*rel);
        }
    }
    let base: HashMap<&str, f64> = single.into_iter().map(|(stat, v)| (stat, mean(&v))).collect();
    let mut pairs: HashMap<(String, String), Vec<f64>> = HashMap::new();
    for (listing, rel) in &rel_rows {
        let mut stats: Vec<&str> = listing.rolls.iter().map(|(stat, _)| stat.as_str()).collect();
        stats.sort_unstable();
        stats.dedup();
        for i in 0..stats.len() {
            for &b in &stats[i + 1..] {
                pairs.entry((stats[i].to_owned(), b.to_owned())).or_default().push(*rel);
            }
        }
    }
    let mut out: Vec<PairSynergy> = pairs
        .into_iter()
        .filter(|(_, rels)| rels.len() >= MIN_STAT_SUPPORT)
        .map(|((a, b), rels)| {
            let best_separate = base.get(a.as_str()).copied().unwrap_or(0.0).max(base.get(b.as_str()).copied().unwrap_or(0.0));
            let synergy = mean(&rels) - best_separate;
            PairSynergy { pair: format!("{a} + {b}"), synergy: round1((synergy.exp() - 1.0) * 100.0), support: rels.len() }
        })
        .collect();
    out.sort_by(|a, b| b.synergy.total_cmp(&a.synergy));
    out.truncate(top);
    out
}

/// A rarity step's median price ratio: port of `market_patterns.py`'s `rarity_steps` dict entry.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct RarityStep {
    pub median_ratio: f64,
    pub archetypes: usize,
}

/// Median price ratio between consecutive rarities of the same item archetype (its id without the
/// trailing `_<rarity><variant>` suffix), keyed `"{a}->{b}"`.
pub fn rarity_steps(listings: &[Listing]) -> BTreeMap<String, RarityStep> {
    let mut by_key: HashMap<(String, i64), Vec<i64>> = HashMap::new();
    for listing in listings {
        if listing.item_count == 1 && listing.rarity != 0 {
            let archetype = listing.item_id.rsplit_once('_').map_or_else(|| listing.item_id.clone(), |(a, _)| a.to_owned());
            by_key.entry((archetype, listing.rarity)).or_default().push(listing.price);
        }
    }
    let mut medians: HashMap<String, BTreeMap<i64, f64>> = HashMap::new();
    for ((archetype, rarity), prices) in by_key {
        if prices.len() >= 3 {
            let prices_f: Vec<f64> = prices.iter().map(|&p| p as f64).collect();
            medians.entry(archetype).or_default().insert(rarity, median(&prices_f));
        }
    }
    let mut ratios: HashMap<(i64, i64), Vec<f64>> = HashMap::new();
    for per_rarity in medians.values() {
        for (&rarity, &price) in per_rarity {
            if let Some(&lower) = per_rarity.get(&(rarity - 1)) {
                ratios.entry((rarity - 1, rarity)).or_default().push(price / lower);
            }
        }
    }
    ratios.into_iter().map(|((a, b), v)| (format!("{a}->{b}"), RarityStep { median_ratio: round2(median(&v)), archetypes: v.len() })).collect()
}

/// Price-ending habits (round numbers, `.99`, repeating digits), as a share of single-item
/// listings, formatted like Python's `f"{pct:.0f}%"`.
pub fn price_habits(listings: &[Listing]) -> BTreeMap<String, String> {
    let prices: Vec<i64> = listings.iter().filter(|l| l.item_count == 1).map(|l| l.price).collect();
    if prices.is_empty() {
        return BTreeMap::new();
    }
    let mut ends: HashMap<&str, usize> = HashMap::new();
    for &p in &prices {
        let bucket = if p % 100 == 0 {
            Some("x00")
        } else if p % 100 == 99 {
            Some("x99")
        } else if p % 50 == 0 {
            Some("x50")
        } else if p % 111 == 0 || p % 1111 == 0 {
            Some("repeating digits")
        } else {
            None
        };
        if let Some(bucket) = bucket {
            *ends.entry(bucket).or_insert(0) += 1;
        }
    }
    ends.into_iter().map(|(k, v)| (k.to_owned(), format!("{:.0}%", v as f64 / prices.len() as f64 * 100.0))).collect()
}

/// Share of single-item listings priced under half their item's median ask.
pub fn lowball_share(listings: &[Listing]) -> String {
    let groups = group_by_item(listings);
    let (mut total, mut low) = (0usize, 0usize);
    for rows in groups.values() {
        let prices: Vec<f64> = rows.iter().map(|r| r.price as f64).collect();
        let typical = median(&prices);
        total += rows.len();
        low += rows.iter().filter(|r| (r.price as f64) < LOWBALL_RATIO * typical).count();
    }
    if total == 0 {
        "n/a".to_owned()
    } else {
        format!("{:.1}%", low as f64 / total as f64 * 100.0)
    }
}

/// One seller's share of listings: port of `market_patterns.py`'s `seller_concentration` list entry.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SellerShare {
    pub seller_share: String,
    pub listings: usize,
}

/// The top sellers by listing count and their share of all (seller-attributed) listings.
pub fn seller_concentration(listings: &[Listing], top: usize) -> Vec<SellerShare> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for l in listings {
        if !l.seller.is_empty() {
            *counts.entry(l.seller.as_str()).or_insert(0) += 1;
        }
    }
    let total: usize = counts.values().sum();
    if total == 0 {
        return Vec::new();
    }
    let mut entries: Vec<(&str, usize)> = counts.into_iter().collect();
    entries.sort_by_key(|a| std::cmp::Reverse(a.1));
    entries.truncate(top);
    entries.into_iter().map(|(_, c)| SellerShare { seller_share: format!("{:.1}%", c as f64 / total as f64 * 100.0), listings: c }).collect()
}

/// [`stat_premiums`] split by item type (weapon / armor / accessory / ...), for types with at
/// least twice [`MIN_ITEM_LISTINGS`] listings.
pub fn stat_premiums_by_type(listings: &[Listing], item_types: &HashMap<String, String>) -> BTreeMap<String, Vec<(String, StatPremium)>> {
    let mut by_type: HashMap<String, Vec<Listing>> = HashMap::new();
    for l in listings {
        let kind = item_types.get(&l.item_id).cloned().unwrap_or_else(|| "other".to_owned());
        by_type.entry(kind).or_default().push(l.clone());
    }
    by_type.into_iter().filter(|(_, rows)| rows.len() >= MIN_ITEM_LISTINGS * 2).map(|(kind, rows)| (kind, stat_premiums(&rows))).collect()
}

/// The full pattern-analysis report: port of `market_patterns.py`'s `analyze`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Report {
    pub stat_premiums_by_type: BTreeMap<String, BTreeMap<String, StatPremium>>,
    pub listings: usize,
    pub items: usize,
    pub stat_premiums: BTreeMap<String, StatPremium>,
    pub good_roll_counts: BTreeMap<u32, GoodRollCount>,
    pub extra_good_roll_factor: Option<f64>,
    pub pair_synergies: Vec<PairSynergy>,
    pub rarity_steps: BTreeMap<String, RarityStep>,
    pub price_habits: BTreeMap<String, String>,
    pub lowball_share: String,
    pub seller_concentration: Vec<SellerShare>,
    pub roll_ranges: BTreeMap<String, BTreeMap<String, (f64, f64, usize)>>,
}

/// Runs every analysis over `listings` and assembles the full report.
pub fn analyze(listings: &[Listing], item_types: &HashMap<String, String>) -> Report {
    let counts = good_roll_counts(listings);
    let items: std::collections::HashSet<&str> = listings.iter().map(|l| l.item_id.as_str()).collect();
    Report {
        stat_premiums_by_type: stat_premiums_by_type(listings, item_types)
            .into_iter()
            .map(|(kind, premiums)| (kind, premiums.into_iter().collect()))
            .collect(),
        listings: listings.len(),
        items: items.len(),
        stat_premiums: stat_premiums(listings).into_iter().collect(),
        extra_good_roll_factor: extra_good_roll_factor(&counts),
        good_roll_counts: counts,
        pair_synergies: pair_synergies(listings, MODEL_PAIRS),
        rarity_steps: rarity_steps(listings),
        price_habits: price_habits(listings),
        lowball_share: lowball_share(listings),
        seller_concentration: seller_concentration(listings, 5),
        roll_ranges: roll_ranges(listings),
    }
}

// --- market_model.json: what pricing learns from the report above (port of `market_model.py`) ---

/// The subset of a [`Report`] pricing actually reads (as a `serde_json::Map` key order, though
/// irrelevant to any reader, follows this list).
const MODEL_KEYS: [&str; 5] = ["stat_premiums", "good_roll_counts", "extra_good_roll_factor", "roll_ranges", "pair_synergies"];
/// Listings carrying a stat pair before its bonus is trusted for pricing; the default `min_support`
/// Python's `pair_bonuses` bakes into its signature. Not re-exported at the crate root: it would
/// collide with `worth_train::MIN_PAIR_SUPPORT`, a different threshold for a different purpose.
pub const MIN_PAIR_SUPPORT: u64 = 10;
const REPLACE_ATTEMPTS: u32 = 5;
const REPLACE_RETRY: Duration = Duration::from_millis(100);
const PAIR_SEPARATOR: &str = " + ";

/// Keeps the keys pricing needs from a full [`Report`], as the JSON saved to `market_model.json`.
pub fn model_from_report(report: &Report) -> serde_json::Value {
    let value = serde_json::to_value(report).expect("Report always serializes");
    let mut out = serde_json::Map::new();
    for key in MODEL_KEYS {
        if let Some(v) = value.get(key) {
            out.insert(key.to_owned(), v.clone());
        }
    }
    serde_json::Value::Object(out)
}

/// Writes `model` to `path` through a temporary file and an atomic rename, so a reader never sees
/// a half-written file. On Windows a concurrent reader can make the rename fail with "access
/// denied"; that's retried briefly (`REPLACE_ATTEMPTS`), matching `market_model.py`'s `_replace`.
pub fn save_model(path: &Path, model: &serde_json::Value) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let tmp_path = dir.join(format!(".market_model.{}.tmp", std::process::id()));
    let written = (|| -> io::Result<()> {
        let file = std::fs::File::create(&tmp_path)?;
        serde_json::to_writer_pretty(&file, model).map_err(io::Error::from)?;
        file.sync_all()
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
        return written;
    }
    replace_with_retry(&tmp_path, path)
}

fn replace_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    for attempt in 0..REPLACE_ATTEMPTS {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt + 1 < REPLACE_ATTEMPTS => {
                std::thread::sleep(REPLACE_RETRY * (attempt + 1));
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the loop above always returns by its last iteration")
}

/// The saved model at `path`, or an empty object when there is none (or it can't be read) —
/// matching `market_model.py`'s tolerant `load_model`.
pub fn load_model(path: &Path) -> serde_json::Value {
    let parsed = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    match parsed {
        Some(v @ serde_json::Value::Object(_)) => v,
        _ => serde_json::Value::Object(serde_json::Map::new()),
    }
}

/// Splits `"A + B"` into exactly two trimmed names, or `None` for anything else (matching
/// Python's `first, second = pair.split(" + ")`, which raises — and is caught — on any other count).
fn split_pair(pair: &str) -> Option<(String, String)> {
    match pair.split(PAIR_SEPARATOR).collect::<Vec<_>>().as_slice() {
        [a, b] => Some((a.trim().to_owned(), b.trim().to_owned())),
        _ => None,
    }
}

/// `{stat pair: percent}` for pairs that sell for more together, well supported and reported in a
/// saved `market_model.json` (tolerant of a malformed or missing `pair_synergies` entry).
pub fn pair_bonuses(model: &serde_json::Value, min_support: u64) -> HashMap<BTreeSet<String>, f64> {
    let mut bonuses = HashMap::new();
    let Some(entries) = model.get("pair_synergies").and_then(|v| v.as_array()) else {
        return bonuses;
    };
    for entry in entries {
        let Some(pair) = entry.get("pair").and_then(|v| v.as_str()).and_then(split_pair) else { continue };
        let Some(percent) = entry.get("synergy").and_then(|v| v.as_f64()) else { continue };
        let Some(support) = entry.get("support").and_then(|v| v.as_u64()) else { continue };
        if support >= min_support && percent > 0.0 {
            bonuses.insert(BTreeSet::from([pair.0, pair.1]), percent);
        }
    }
    bonuses
}

/// Learned share of an extra good roll's premium (0..1) from a saved `market_model.json`, else
/// `default` (also on a missing or non-numeric value).
pub fn extra_roll_share(model: &serde_json::Value, default: f64) -> f64 {
    model.get("extra_good_roll_factor").and_then(|v| v.as_f64()).map_or(default, |x| x.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_differs_from_worth_roll_quality_on_a_degenerate_range() {
        assert_eq!(percentile(17.0, 10.0, 20.0), 0.7);
        assert_eq!(percentile(5.0, 5.0, 5.0), 1.0);
    }

    #[test]
    fn median_and_round_helpers() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(round1(12.34), 12.3);
        assert_eq!(round2(0.125 + 1e-9), 0.13);
    }
}
