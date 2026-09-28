//! Builds the sparse one-hot design matrix a row of listings fits against: port of
//! `worth_model.py`'s `_design`.

use std::collections::{HashMap, HashSet};

use crate::worth::roll_quality;

use super::rows::{pair_key, roll_names, tier, RangeMap, Row};
use super::sparse::{Csr, CsrBuilder};

/// A fitted design matrix and the coefficient name for each of its columns.
pub(crate) struct Design {
    pub(crate) matrix: Csr,
    pub(crate) names: Vec<String>,
}

/// The item's own range for `stat`, falling back to its `"{slot}|{rarity}"` group's range: port of
/// `WorthModel._range` (used here as the training-time "probe" and again in `super::finish`).
pub(crate) fn range_of(
    ranges: &RangeMap,
    group_ranges: &RangeMap,
    item_id: &str,
    slot: &str,
    rarity: i64,
    stat: &str,
) -> Option<[f64; 2]> {
    ranges
        .get(item_id)
        .and_then(|stats| stats.get(stat))
        .or_else(|| group_ranges.get(&format!("{slot}|{rarity}")).and_then(|stats| stats.get(stat)))
        .copied()
}

/// Assigns a stable column index to each coefficient name, in first-seen order (the order itself
/// is arbitrary — only the name <-> index mapping within one design matrix matters).
#[derive(Default)]
struct Indexer {
    index: HashMap<String, u32>,
    names: Vec<String>,
}

impl Indexer {
    fn id(&mut self, name: String) -> u32 {
        if let Some(&i) = self.index.get(&name) {
            return i;
        }
        let i = self.names.len() as u32;
        self.index.insert(name.clone(), i);
        self.names.push(name);
        i
    }
}

/// The design matrix for `rows`: one row per listing, one column per coefficient name any listing
/// touches (group, item, age bucket, per-roll tier/quality terms, and frequent stat pairs).
pub(crate) fn build_design(rows: &[Row], ranges: &RangeMap, group_ranges: &RangeMap, pairs: &[String]) -> Design {
    let pair_set: HashSet<&str> = pairs.iter().map(String::as_str).collect();
    let mut idx = Indexer::default();
    let mut builder = CsrBuilder::new();
    for row in rows {
        let mut entries: Vec<(u32, f64)> =
            vec![(idx.id(format!("g|{}|{}", row.slot, row.rarity)), 1.0), (idx.id(format!("i|{}", row.item_id)), 1.0)];
        if let Some(age) = row.age {
            entries.push((idx.id(format!("a|{age}")), 1.0));
        }
        for (stat, value) in &row.rolls {
            let quality = range_of(ranges, group_ranges, &row.item_id, &row.slot, row.rarity, stat)
                .map_or(0.5, |[lo, hi]| roll_quality(*value, lo, hi));
            for name in roll_names(stat, &row.slot, row.rarity, tier(quality)) {
                entries.push((idx.id(name), 1.0));
            }
            entries.push((idx.id(format!("q|{stat}|{}", row.rarity)), quality - 0.5));
        }
        let mut stats: Vec<&str> = row.rolls.iter().map(|(stat, _)| stat.as_str()).collect();
        stats.sort_unstable();
        stats.dedup();
        for i in 0..stats.len() {
            for &b in &stats[i + 1..] {
                let key = pair_key(stats[i], b);
                if pair_set.contains(key.as_str()) {
                    entries.push((idx.id(format!("p|{key}")), 1.0));
                }
            }
        }
        builder.push_row(entries);
    }
    Design { matrix: builder.finish(), names: idx.names }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn row(item_id: &str, slot: &str, rarity: i64, rolls: &[(&str, f64)]) -> Row {
        Row {
            item_id: item_id.to_owned(),
            rarity,
            slot: slot.to_owned(),
            price: 100.0,
            rolls: rolls.iter().map(|(s, v)| ((*s).to_owned(), *v)).collect(),
            age: Some(0),
        }
    }

    #[test]
    fn every_row_gets_a_group_item_and_age_column() {
        let rows = vec![row("Helm_5001", "Head", 5, &[])];
        let design = build_design(&rows, &HashMap::new(), &HashMap::new(), &[]);
        assert_eq!(design.names, vec!["g|Head|5", "i|Helm_5001", "a|0"]);
        assert_eq!(design.matrix.matvec(&[1.0, 1.0, 1.0]), vec![3.0]);
    }

    #[test]
    fn a_roll_with_no_known_range_gets_the_middle_tier_and_zero_centred_quality() {
        let rows = vec![row("Helm_5001", "Head", 5, &[("Luck", 2.0)])];
        let design = build_design(&rows, &HashMap::new(), &HashMap::new(), &[]);
        assert!(design.names.contains(&"r|Luck|1".to_string()));
        assert!(design.names.contains(&"q|Luck|5".to_string()));
        // roll_quality with no bounds is 0.5, so the quality column (centred on 0.5) is 0.
        let q_col = design.names.iter().position(|n| n == "q|Luck|5").unwrap();
        let mut probe = vec![0.0; design.names.len()];
        probe[q_col] = 1.0;
        assert_eq!(design.matrix.matvec(&probe)[0], 0.0);
    }

    #[test]
    fn only_frequent_pairs_get_a_column() {
        let rows = vec![row("Helm_5001", "Head", 5, &[("Luck", 2.0), ("Vigor", 2.0)])];
        let with_pair = build_design(&rows, &HashMap::new(), &HashMap::new(), &["Luck+Vigor".to_string()]);
        assert!(with_pair.names.contains(&"p|Luck+Vigor".to_string()));
        let without_pair = build_design(&rows, &HashMap::new(), &HashMap::new(), &[]);
        assert!(!without_pair.names.iter().any(|n| n.starts_with("p|")));
    }

    #[test]
    fn range_of_prefers_the_items_own_range_over_the_group() {
        let mut ranges = RangeMap::new();
        ranges.insert("Helm_5001".to_string(), HashMap::from([("Luck".to_string(), [1.0, 5.0])]));
        let mut group_ranges = RangeMap::new();
        group_ranges.insert("Head|5".to_string(), HashMap::from([("Luck".to_string(), [0.0, 10.0])]));
        assert_eq!(range_of(&ranges, &group_ranges, "Helm_5001", "Head", 5, "Luck"), Some([1.0, 5.0]));
        assert_eq!(range_of(&ranges, &group_ranges, "OtherHelm_5001", "Head", 5, "Luck"), Some([0.0, 10.0]));
        assert_eq!(range_of(&ranges, &group_ranges, "OtherHelm_5001", "Head", 5, "Vigor"), None);
    }
}
