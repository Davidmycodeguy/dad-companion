//! Turns raw listings into fitting rows, plus the small pure helpers shared by [`super::design`]
//! and [`super::finish`]: a roll's tier within its range, per-item/group roll ranges, and which
//! stat pairs are common enough to earn their own term.

use std::collections::HashMap;

use crate::history::rarity_of;

use super::TrainListing;

/// Slot used for an item the catalog has no `slot_type` for.
pub(crate) const UNKNOWN_SLOT: &str = "?";
const MIN_RANGE_VALUES: usize = 3;
/// A roll's tier is where it sits in its range: weak / mid / strong.
const TIER_EDGES: (f64, f64) = (1.0 / 3.0, 2.0 / 3.0);
/// How long a listing had been up when first seen: 0 (fresh) .. 3 (old).
const AGE_EDGES_DAYS: [f64; 3] = [1.0, 3.0, 5.0];

/// One listing ready to fit: per-unit price, its slot, and its age bucket.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub(crate) item_id: String,
    pub(crate) rarity: i64,
    pub(crate) slot: String,
    /// Price for one unit (a stack's price divided by its count).
    pub(crate) price: f64,
    pub(crate) rolls: Vec<(String, f64)>,
    pub(crate) age: Option<u8>,
}

/// Roll ranges keyed by item id, or by `"{slot}|{rarity}"` for the group fallback.
pub(crate) type RangeMap = HashMap<String, HashMap<String, [f64; 2]>>;

/// Where `quality` sits: 0 (weak) .. 2 (strong).
pub(crate) fn tier(quality: f64) -> u8 {
    if quality < TIER_EDGES.0 {
        0
    } else if quality < TIER_EDGES.1 {
        1
    } else {
        2
    }
}

/// The three coefficient names a roll of `stat` at this `tier` contributes to.
pub(crate) fn roll_names(stat: &str, slot: &str, rarity: i64, tier: u8) -> [String; 3] {
    [format!("r|{stat}|{tier}"), format!("rr|{stat}|{rarity}|{tier}"), format!("rs|{stat}|{slot}|{rarity}|{tier}")]
}

/// The two stats of a pair, alphabetical so the same pair always yields the same key.
pub(crate) fn pair_key(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a}+{b}")
    } else {
        format!("{b}+{a}")
    }
}

/// How long a listing had been up when first seen, bucketed; `None` when unknown.
pub(crate) fn age_bucket(days: Option<f64>) -> Option<u8> {
    let days = days?;
    Some(AGE_EDGES_DAYS.iter().filter(|&&edge| days >= edge).count() as u8)
}

/// Listings with a usable price, turned into fitting rows (unpriced listings teach nothing).
pub(crate) fn rows(listings: &[TrainListing], item_slots: &HashMap<String, String>) -> Vec<Row> {
    listings
        .iter()
        .filter(|listing| listing.price > 0)
        .map(|listing| {
            let count = listing.item_count.max(1);
            let rarity = if listing.rarity != 0 { listing.rarity } else { rarity_of(&listing.item_id) };
            let slot = item_slots
                .get(&listing.item_id)
                .map(String::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or(UNKNOWN_SLOT)
                .to_owned();
            Row {
                item_id: listing.item_id.clone(),
                rarity,
                slot,
                price: listing.price as f64 / count as f64,
                rolls: listing.rolls.clone(),
                age: age_bucket(listing.age_days),
            }
        })
        .collect()
}

/// Per-item and per-group (`"{slot}|{rarity}"`) `[min, max]` for each stat, once at least
/// [`MIN_RANGE_VALUES`] values have been seen (else a roll's quality there is unknown).
pub(crate) fn ranges(rows: &[Row]) -> (RangeMap, RangeMap) {
    let mut per_item: HashMap<String, HashMap<String, Vec<f64>>> = HashMap::new();
    let mut per_group: HashMap<String, HashMap<String, Vec<f64>>> = HashMap::new();
    for row in rows {
        for (stat, value) in &row.rolls {
            per_item.entry(row.item_id.clone()).or_default().entry(stat.clone()).or_default().push(*value);
            let group = format!("{}|{}", row.slot, row.rarity);
            per_group.entry(group).or_default().entry(stat.clone()).or_default().push(*value);
        }
    }
    (span(per_item), span(per_group))
}

fn span(table: HashMap<String, HashMap<String, Vec<f64>>>) -> RangeMap {
    table
        .into_iter()
        .map(|(key, stats)| {
            let spans = stats
                .into_iter()
                .filter(|(_, values)| values.len() >= MIN_RANGE_VALUES)
                .map(|(stat, values)| {
                    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
                    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    (stat, [lo, hi])
                })
                .collect();
            (key, spans)
        })
        .collect()
}

/// Stat pairs seen together on at least `min_support` listings, sorted.
pub(crate) fn frequent_pairs(rows: &[Row], min_support: usize) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for row in rows {
        let mut stats: Vec<&str> = row.rolls.iter().map(|(stat, _)| stat.as_str()).collect();
        stats.sort_unstable();
        stats.dedup();
        for i in 0..stats.len() {
            for &b in &stats[i + 1..] {
                *counts.entry(pair_key(stats[i], b)).or_insert(0) += 1;
            }
        }
    }
    let mut pairs: Vec<String> = counts.into_iter().filter(|(_, n)| *n >= min_support).map(|(pair, _)| pair).collect();
    pairs.sort();
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_thirds() {
        assert_eq!((tier(0.0), tier(0.33), tier(0.34), tier(0.66), tier(0.67), tier(1.0)), (0, 0, 1, 1, 2, 2));
    }

    #[test]
    fn pair_key_is_alphabetical_regardless_of_argument_order() {
        assert_eq!(pair_key("Vigor", "Luck"), "Luck+Vigor");
        assert_eq!(pair_key("Luck", "Vigor"), "Luck+Vigor");
    }

    #[test]
    fn age_bucket_steps_at_the_edges() {
        assert_eq!(age_bucket(None), None);
        assert_eq!(age_bucket(Some(0.0)), Some(0));
        assert_eq!(age_bucket(Some(1.0)), Some(1));
        assert_eq!(age_bucket(Some(4.0)), Some(2));
        assert_eq!(age_bucket(Some(5.0)), Some(3));
    }

    fn listing(item_id: &str, price: i64, rolls: &[(&str, f64)]) -> TrainListing {
        TrainListing {
            item_id: item_id.to_owned(),
            rarity: 5,
            price,
            item_count: 1,
            rolls: rolls.iter().map(|(s, v)| ((*s).to_owned(), *v)).collect(),
            age_days: None,
        }
    }

    #[test]
    fn rows_skips_unpriced_listings_and_divides_by_count() {
        let mut stack = listing("Sword_5001", 300, &[]);
        stack.item_count = 3;
        let listings = vec![listing("Sword_5001", 0, &[]), stack];
        let out = rows(&listings, &HashMap::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].price, 100.0);
        assert_eq!(out[0].slot, UNKNOWN_SLOT);
    }

    #[test]
    fn ranges_need_a_minimum_of_values_and_fall_back_to_the_group() {
        let listings: Vec<TrainListing> =
            (0..3).map(|i| listing("Helm_5001", 100, &[("Luck", i as f64)])).collect();
        let rows = rows(&listings, &HashMap::new());
        let (per_item, per_group) = ranges(&rows);
        assert_eq!(per_item["Helm_5001"]["Luck"], [0.0, 2.0]);
        assert_eq!(per_group[&format!("{UNKNOWN_SLOT}|5")]["Luck"], [0.0, 2.0]);

        // The item key is kept (Python's dict comprehension only drops thin *stats*), but with no
        // stat meeting MIN_RANGE_VALUES its range table is empty.
        let too_few = rows_from(&[listing("Cap_5001", 100, &[("Luck", 1.0)])]);
        assert!(ranges(&too_few).0.get("Cap_5001").is_none_or(|stats| stats.is_empty()));
    }

    fn rows_from(listings: &[TrainListing]) -> Vec<Row> {
        rows(listings, &HashMap::new())
    }

    #[test]
    fn frequent_pairs_need_minimum_support_and_ignore_order() {
        let listings: Vec<TrainListing> = (0..5)
            .map(|_| listing("Helm_5001", 100, &[("Vigor", 1.0), ("Luck", 1.0)]))
            .collect();
        let rows = rows_from(&listings);
        assert_eq!(frequent_pairs(&rows, 5), vec!["Luck+Vigor".to_string()]);
        assert!(frequent_pairs(&rows, 6).is_empty());
    }
}
