//! Post-fit steps: per-item calibration, unrolled-item medians, and the average roll effect /
//! residual spreads saved for [`crate::worth::WorthModel::predict`]'s bands and confidence.
//! Port of `worth_model.py`'s `_calibrate_items`, `_unrolled_medians` and `_finish`.

use std::collections::HashMap;

use crate::worth::roll_quality;

use super::design::range_of;
use super::rows::{tier, RangeMap, Row};

const MIN_SPREAD_ROWS: usize = 5;
const MIN_RANGE_VALUES: usize = 3;
const LOW_QUANTILE: f64 = 0.2;
const LOW_MIN_ROWS: usize = 15;

/// The middle value (the mean of the two middle ones for an even count), matching Python's
/// `statistics.median`.
pub(crate) fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return 0.0; // Every call site here only ever passes a non-empty slice.
    }
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// The median absolute deviation from the median.
pub(crate) fn mad(values: &[f64]) -> f64 {
    let centre = median(values);
    let deviations: Vec<f64> = values.iter().map(|v| (v - centre).abs()).collect();
    median(&deviations)
}

/// The value at the `q` quantile by truncating index (not interpolating), matching Python's
/// `_quantile`: `sorted(values)[min(int(len(values) * q), len(values) - 1)]`.
pub(crate) fn quantile(values: &[f64], q: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let idx = ((v.len() as f64) * q) as usize;
    v[idx.min(v.len() - 1)]
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn default_low_offset() -> f64 {
    0.81_f64.ln()
}

/// The price effect of one roll: the sum of its tier/rarity/slot terms plus its quality slope,
/// read straight from a fitted coefficient map. Mirrors `WorthModel`'s private `_roll_effect`,
/// which isn't reachable from here (training and prediction are split across files).
pub(crate) fn roll_effect(coef: &HashMap<String, f64>, stat: &str, slot: &str, rarity: i64, quality: f64) -> f64 {
    let t = tier(quality);
    let get = |name: &str| coef.get(name).copied().unwrap_or(0.0);
    get(&format!("r|{stat}|{t}"))
        + get(&format!("rr|{stat}|{rarity}|{t}"))
        + get(&format!("rs|{stat}|{slot}|{rarity}|{t}"))
        + get(&format!("q|{stat}|{rarity}")) * (quality - 0.5)
}

/// Shifts each well-seen item's own coefficient to its median residual (asks are skewed; the
/// median is the norm) and returns residuals adjusted for that shift.
pub(crate) fn calibrate_items(coef: &mut HashMap<String, f64>, rows: &[Row], residuals: &[f64]) -> Vec<f64> {
    let mut by_item: HashMap<&str, Vec<f64>> = HashMap::new();
    for (row, &res) in rows.iter().zip(residuals) {
        by_item.entry(row.item_id.as_str()).or_default().push(res);
    }
    let shift: HashMap<&str, f64> = by_item
        .iter()
        .filter(|(_, v)| v.len() >= MIN_SPREAD_ROWS)
        .map(|(&item, v)| (item, median(v)))
        .collect();
    for (&item, &delta) in &shift {
        let key = format!("i|{item}");
        let current = coef.get(&key).copied().unwrap_or(0.0);
        coef.insert(key, current + delta);
    }
    rows.iter().zip(residuals).map(|(row, &res)| res - shift.get(row.item_id.as_str()).copied().unwrap_or(0.0)).collect()
}

/// The median ask of items with no random rolls at all (their fit is unreliable; a plain median
/// beats it), once enough of them have been seen.
pub(crate) fn unrolled_medians(rows: &[Row]) -> HashMap<String, f64> {
    let mut prices: HashMap<&str, Vec<f64>> = HashMap::new();
    for row in rows.iter().filter(|r| r.rolls.is_empty()) {
        prices.entry(row.item_id.as_str()).or_default().push(row.price);
    }
    prices.into_iter().filter(|(_, v)| v.len() >= MIN_RANGE_VALUES).map(|(k, v)| (k.to_owned(), median(&v))).collect()
}

/// Everything computed after the fit and calibration: average roll effect per rarity (for
/// explanations) and residual spreads / low-price offsets (for bands and confidence).
pub(crate) struct FinishOutput {
    pub(crate) avg_roll: HashMap<String, f64>,
    pub(crate) spread: HashMap<String, f64>,
    pub(crate) group_spread: HashMap<String, f64>,
    pub(crate) global_spread: f64,
    pub(crate) low_offset: HashMap<String, f64>,
    pub(crate) group_low_offset: HashMap<String, f64>,
    pub(crate) global_low_offset: f64,
}

pub(crate) fn finish(
    rows: &[Row],
    residuals: &[f64],
    coef: &HashMap<String, f64>,
    ranges: &RangeMap,
    group_ranges: &RangeMap,
) -> FinishOutput {
    let mut effects: HashMap<String, Vec<f64>> = HashMap::new();
    for row in rows {
        for (stat, value) in &row.rolls {
            let quality = range_of(ranges, group_ranges, &row.item_id, &row.slot, row.rarity, stat)
                .map_or(0.5, |[lo, hi]| roll_quality(*value, lo, hi));
            effects.entry(row.rarity.to_string()).or_default().push(roll_effect(coef, stat, &row.slot, row.rarity, quality));
        }
    }
    let avg_roll = effects.into_iter().map(|(rarity, v)| (rarity, mean(&v))).collect();

    let mut by_item: HashMap<String, Vec<f64>> = HashMap::new();
    let mut by_group: HashMap<String, Vec<f64>> = HashMap::new();
    for (row, &res) in rows.iter().zip(residuals) {
        by_item.entry(row.item_id.clone()).or_default().push(res);
        by_group.entry(format!("{}|{}", row.slot, row.rarity)).or_default().push(res);
    }
    let spread = by_item.iter().filter(|(_, v)| v.len() >= MIN_SPREAD_ROWS).map(|(k, v)| (k.clone(), mad(v))).collect();
    let low_offset =
        by_item.iter().filter(|(_, v)| v.len() >= LOW_MIN_ROWS).map(|(k, v)| (k.clone(), quantile(v, LOW_QUANTILE))).collect();
    let group_low_offset =
        by_group.iter().filter(|(_, v)| v.len() >= LOW_MIN_ROWS).map(|(k, v)| (k.clone(), quantile(v, LOW_QUANTILE))).collect();
    let global_low_offset = if residuals.is_empty() { default_low_offset() } else { quantile(residuals, LOW_QUANTILE) };
    let group_spread = by_group.iter().filter(|(_, v)| v.len() >= MIN_SPREAD_ROWS).map(|(k, v)| (k.clone(), mad(v))).collect();
    let global_spread = if residuals.is_empty() { 0.3 } else { mad(residuals) };
    FinishOutput { avg_roll, spread, group_spread, global_spread, low_offset, group_low_offset, global_low_offset }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_mad_match_known_values() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(mad(&[1.0, 1.0, 1.0, 1.0]), 0.0);
        assert_eq!(mad(&[1.0, 2.0, 3.0, 4.0, 5.0]), 1.0);
    }

    #[test]
    fn quantile_truncates_rather_than_interpolates() {
        let values = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(quantile(&values, 0.2), 20.0); // index floor(5*0.2)=1
        assert_eq!(quantile(&values, 0.0), 10.0);
        assert_eq!(quantile(&values, 0.999), 50.0); // clamped to the last index
    }

    fn row(item_id: &str, price: f64, rolls: &[(&str, f64)]) -> Row {
        Row {
            item_id: item_id.to_owned(),
            rarity: 5,
            slot: "Head".to_owned(),
            price,
            rolls: rolls.iter().map(|(s, v)| ((*s).to_owned(), *v)).collect(),
            age: Some(0),
        }
    }

    #[test]
    fn calibrate_items_shifts_well_seen_items_to_their_median_residual_and_leaves_others_alone() {
        let rows = vec![
            row("Helm_5001", 100.0, &[]),
            row("Helm_5001", 100.0, &[]),
            row("Helm_5001", 100.0, &[]),
            row("Helm_5001", 100.0, &[]),
            row("Helm_5001", 100.0, &[]),
            row("Rare_5001", 50.0, &[]),
        ];
        let residuals = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.9];
        let mut coef = HashMap::new();
        let adjusted = calibrate_items(&mut coef, &rows, &residuals);
        assert!((coef["i|Helm_5001"] - 0.3).abs() < 1e-9); // median of 0.1..0.5
        assert!(!coef.contains_key("i|Rare_5001")); // fewer than MIN_SPREAD_ROWS rows
        assert!((adjusted[0] - -0.2).abs() < 1e-9); // 0.1 - 0.3
        assert!((adjusted[5] - 0.9).abs() < 1e-9); // untouched: no shift for this item
    }

    #[test]
    fn unrolled_medians_need_a_minimum_of_listings_and_ignore_rolled_items() {
        let rows = vec![
            row("Coin", 100.0, &[]),
            row("Coin", 110.0, &[]),
            row("Coin", 90.0, &[]),
            row("Rare", 500.0, &[]),
            row("Helm_5001", 200.0, &[("Luck", 1.0)]),
        ];
        let medians = unrolled_medians(&rows);
        assert_eq!(medians.get("Coin"), Some(&100.0));
        assert_eq!(medians.get("Rare"), None);
        assert_eq!(medians.get("Helm_5001"), None);
    }
}
