//! Finds listings of the same item closest to a given set of rolls: port of `worth_model.py`'s
//! `similar`, used to show comparable real asks alongside a prediction.

use std::collections::HashMap;

use crate::worth::WorthModel;

/// A listing to compare against, for [`similar`].
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub item_id: String,
    pub rolls: Vec<(String, f64)>,
    pub price: f64,
}

/// Listings of `item_id` among `candidates` closest to `rolls`: shared stats with close quality
/// first, cheaper first on a tie.
pub fn similar(model: &WorthModel, item_id: &str, rolls: &[(String, f64)], candidates: &[Candidate], limit: usize) -> Vec<Candidate> {
    let ours: HashMap<&str, f64> =
        rolls.iter().map(|(stat, value)| (stat.as_str(), model.quality(item_id, stat, *value, None, None))).collect();
    let mut scored: Vec<(f64, f64, &Candidate)> = candidates
        .iter()
        .filter(|c| c.item_id == item_id)
        .map(|c| {
            let theirs: HashMap<&str, f64> =
                c.rolls.iter().map(|(stat, value)| (stat.as_str(), model.quality(item_id, stat, *value, None, None))).collect();
            let missing: f64 = ours.iter().map(|(stat, q)| theirs.get(stat).map_or(1.0, |tq| (q - tq).abs())).sum();
            let extra = theirs.keys().filter(|stat| !ours.contains_key(*stat)).count() as f64;
            (missing + 0.5 * extra, c.price, c)
        })
        .collect();
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, c)| c.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(item_id: &str, price: f64, rolls: &[(&str, f64)]) -> Candidate {
        Candidate { item_id: item_id.to_owned(), price, rolls: rolls.iter().map(|(s, v)| ((*s).to_owned(), *v)).collect() }
    }

    #[test]
    fn same_three_stats_beats_two_exact_matches_plus_a_stranger() {
        // A model with no market data at all: every roll's quality falls back to 0.5, so distance
        // is decided purely by which stats overlap (this mirrors the shape of the Python test
        // without needing a trained model just to exercise the ordering/tie-break logic).
        let model = WorthModel::from_json(
            r#"{"version":1,"intercept":0.0,"coef":{},"ranges":{},"group_ranges":{},"pairs":[],"slots":{},
                "support":{},"listings":0,"medians":{},"avg_roll":{},"spread":{},"group_spread":{},
                "global_spread":0.3,"low_offset":{},"group_low_offset":{},"global_low_offset":-0.21}"#,
        )
        .unwrap();
        let candidates = vec![
            candidate("Helm_5001", 900.0, &[("Strength", 3.0), ("Vigor", 2.0), ("Luck", 1.0)]),
            candidate("Helm_5001", 400.0, &[("Knowledge", 1.0), ("Will", 1.0), ("Luck", 1.0)]),
            candidate("Helm_5001", 700.0, &[("Strength", 2.0), ("Vigor", 2.0), ("Will", 1.0)]),
            candidate("Cap_5001", 999.0, &[("Strength", 3.0), ("Vigor", 2.0), ("Luck", 1.0)]),
        ];
        let found = similar(&model, "Helm_5001", &[("Strength".into(), 3.0), ("Vigor".into(), 2.0), ("Will".into(), 2.0)], &candidates, 2);
        assert_eq!(found.iter().map(|c| c.price).collect::<Vec<_>>(), vec![700.0, 900.0]);
    }

    #[test]
    fn limit_caps_the_number_returned() {
        let model = WorthModel::from_json(
            r#"{"version":1,"intercept":0.0,"coef":{},"ranges":{},"group_ranges":{},"pairs":[],"slots":{},
                "support":{},"listings":0,"medians":{},"avg_roll":{},"spread":{},"group_spread":{},
                "global_spread":0.3,"low_offset":{},"group_low_offset":{},"global_low_offset":-0.21}"#,
        )
        .unwrap();
        let candidates: Vec<Candidate> = (0..5).map(|i| candidate("Helm_5001", i as f64, &[])).collect();
        assert_eq!(similar(&model, "Helm_5001", &[], &candidates, 2).len(), 2);
    }
}
