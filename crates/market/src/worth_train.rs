//! Trains the Item Worth model (see `crate::worth` for what it predicts and how): the same
//! additive log-price ridge regression `worth_model.py`'s `train` fits, ported so the app can
//! retrain as it records new listings. Saves in the JSON `worth::WorthModel::from_json` loads.
//!
//! log(price) = group(slot, rarity) + item
//!              + Σ rolls [ e(stat, tier) + e(stat, rarity, tier) + e(stat, slot, rarity, tier) ]
//!              + Σ pairs e(stat A + stat B)
//!
//! Fitting: a sparse one-hot design matrix ([`sparse::Csr`]) and ridge regression solved the way
//! scikit-learn's `solver="sparse_cg"` does for sparse input — conjugate gradient on the normal
//! equations, with the intercept handled by centering (see [`ridge`]) rather than by penalizing an
//! intercept column. A robust refit drops listings far from the first fit (lowballs, wild asks).

mod db;
mod design;
mod evaluate;
mod finish;
mod ridge;
mod rows;
mod similar;
mod sparse;

use std::collections::{BTreeMap, HashMap, HashSet};

pub use db::{load_listings, DbError};
pub use evaluate::{evaluate, evaluate_default, EvalReport};
pub use similar::{similar, Candidate};

use rows::UNKNOWN_SLOT;

/// Model format version; matches `crate::worth`'s `MODEL_VERSION`.
pub const MODEL_VERSION: u32 = 1;
/// Listings carrying a stat pair before it gets its own term.
pub const MIN_PAIR_SUPPORT: usize = 50;
const RIDGE_ALPHA: f64 = 1.0;
/// Listings farther than this many (scaled) MADs from the first fit are dropped for the refit.
const OUTLIER_MADS: f64 = 3.0;
const MAD_TO_SIGMA: f64 = 1.4826;
/// Coefficient magnitudes at or below this are treated as zero (and left out of the saved model).
const COEF_EPSILON: f64 = 1e-9;

/// A market listing to train on: every listing `MarketHistory` has recorded, plus how long it had
/// been up when first seen.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainListing {
    pub item_id: String,
    /// 0 when not known (falls back to [`crate::history::rarity_of`]).
    pub rarity: i64,
    /// Price of the whole listing (all `item_count` items).
    pub price: i64,
    pub item_count: i64,
    pub rolls: Vec<(String, f64)>,
    /// How long the listing had been up when first seen: `LISTING_DAYS - (expires_at -
    /// first_seen) / DAY_S`, clamped at 0. `None` when unknown (treated as fresh).
    pub age_days: Option<f64>,
}

/// Tuning knobs for [`train`]; `TrainOptions::default()` matches `worth_model.py`'s defaults.
#[derive(Debug, Clone, Copy)]
pub struct TrainOptions {
    pub min_pair_support: usize,
    pub alpha: f64,
}

impl Default for TrainOptions {
    fn default() -> Self {
        Self { min_pair_support: MIN_PAIR_SUPPORT, alpha: RIDGE_ALPHA }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TrainError {
    #[error("no market listings to learn from yet")]
    NoListings,
}

/// A fitted model: the same fields `worth.rs`'s `Data` deserializes, kept as native maps so
/// [`TrainedModel::predict_value`] (used by [`evaluate`], and useful on its own — see its doc)
/// needs no JSON round trip. [`TrainedModel::to_json`] gives the saved-model form.
pub struct TrainedModel {
    intercept: f64,
    coef: HashMap<String, f64>,
    ranges: rows::RangeMap,
    group_ranges: rows::RangeMap,
    pairs: Vec<String>,
    slots: HashMap<String, String>,
    support: HashMap<String, u64>,
    listings: u64,
    medians: HashMap<String, f64>,
    avg_roll: HashMap<String, f64>,
    spread: HashMap<String, f64>,
    group_spread: HashMap<String, f64>,
    global_spread: f64,
    low_offset: HashMap<String, f64>,
    group_low_offset: HashMap<String, f64>,
    global_low_offset: f64,
}

/// The JSON shape `crate::worth::WorthModel::from_json` deserializes (field names must match
/// exactly); `BTreeMap`s just so a saved file's key order is deterministic.
#[derive(serde::Serialize)]
struct ModelJson {
    version: u32,
    intercept: f64,
    coef: BTreeMap<String, f64>,
    ranges: BTreeMap<String, BTreeMap<String, [f64; 2]>>,
    group_ranges: BTreeMap<String, BTreeMap<String, [f64; 2]>>,
    pairs: Vec<String>,
    slots: BTreeMap<String, String>,
    support: BTreeMap<String, u64>,
    listings: u64,
    medians: BTreeMap<String, f64>,
    avg_roll: BTreeMap<String, f64>,
    spread: BTreeMap<String, f64>,
    group_spread: BTreeMap<String, f64>,
    global_spread: f64,
    low_offset: BTreeMap<String, f64>,
    group_low_offset: BTreeMap<String, f64>,
    global_low_offset: f64,
}

fn to_btree(map: rows::RangeMap) -> BTreeMap<String, BTreeMap<String, [f64; 2]>> {
    map.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect()
}

impl TrainedModel {
    /// The saved-model JSON `crate::worth::WorthModel::from_json` loads.
    pub fn to_json(&self) -> serde_json::Value {
        let json = ModelJson {
            version: MODEL_VERSION,
            intercept: self.intercept,
            coef: self.coef.clone().into_iter().collect(),
            ranges: to_btree(self.ranges.clone()),
            group_ranges: to_btree(self.group_ranges.clone()),
            pairs: self.pairs.clone(),
            slots: self.slots.clone().into_iter().collect(),
            support: self.support.clone().into_iter().collect(),
            listings: self.listings,
            medians: self.medians.clone().into_iter().collect(),
            avg_roll: self.avg_roll.clone().into_iter().collect(),
            spread: self.spread.clone().into_iter().collect(),
            group_spread: self.group_spread.clone().into_iter().collect(),
            global_spread: self.global_spread,
            low_offset: self.low_offset.clone().into_iter().collect(),
            group_low_offset: self.group_low_offset.clone().into_iter().collect(),
            global_low_offset: self.global_low_offset,
        };
        serde_json::to_value(json).expect("ModelJson always serializes")
    }

    /// What a fresh (or `age_days`-old) listing of `item_id` with `rolls` sells for, per unit.
    /// The age-aware core of `worth::WorthModel::predict` — needed because that method (fixed;
    /// not training's to change) always predicts at age 0 bucket "fresh" — kept minimal (no
    /// bands, confidence or explanations) since [`evaluate`] only needs the value; also handy on
    /// its own for asking what the model learned an older listing to be worth.
    pub fn predict_value(&self, item_id: &str, rolls: &[(String, f64)], slot: Option<&str>, age_days: Option<f64>) -> f64 {
        let rarity = crate::history::rarity_of(item_id);
        let slot = self.slots.get(item_id).map(String::as_str).or(slot).filter(|s| !s.is_empty()).unwrap_or(UNKNOWN_SLOT);
        let coef = |name: &str| self.coef.get(name).copied().unwrap_or(0.0);
        let age = rows::age_bucket(age_days).unwrap_or(0);
        let mut log_value = self.intercept + coef(&format!("g|{slot}|{rarity}")) + coef(&format!("i|{item_id}")) + coef(&format!("a|{age}"));
        if rolls.is_empty() {
            if let Some(&median) = self.medians.get(item_id).filter(|m| **m > 0.0) {
                log_value = median.ln();
            }
        }
        for (stat, value) in rolls {
            let quality = design::range_of(&self.ranges, &self.group_ranges, item_id, slot, rarity, stat)
                .map_or(0.5, |[lo, hi]| crate::worth::roll_quality(*value, lo, hi));
            log_value += finish::roll_effect(&self.coef, stat, slot, rarity, quality);
        }
        let stats: HashSet<&str> = rolls.iter().map(|(s, _)| s.as_str()).collect();
        for pair in &self.pairs {
            if pair.split('+').all(|p| stats.contains(p)) {
                log_value += coef(&format!("p|{pair}"));
            }
        }
        log_value.exp()
    }
}

/// Fits the model on market listings; `item_slots` maps item id -> slot name (items.json
/// `slot_type`). Call [`TrainedModel::to_json`] for the form `WorthModel::from_json` loads.
pub fn train(
    listings: &[TrainListing],
    item_slots: &HashMap<String, String>,
    opts: TrainOptions,
) -> Result<TrainedModel, TrainError> {
    let all_rows = rows::rows(listings, item_slots);
    if all_rows.is_empty() {
        return Err(TrainError::NoListings);
    }
    let (ranges, group_ranges) = rows::ranges(&all_rows);
    let pairs = rows::frequent_pairs(&all_rows, opts.min_pair_support);

    let mut rows_used = all_rows;
    let built = design::build_design(&rows_used, &ranges, &group_ranges, &pairs);
    let mut names = built.names;
    let mut targets: Vec<f64> = rows_used.iter().map(|r| r.price.ln()).collect();
    let mut fit = ridge::fit_ridge(&built.matrix, &targets, opts.alpha);
    let mut residuals: Vec<f64> =
        targets.iter().zip(ridge::predict(&built.matrix, &fit)).map(|(t, p)| t - p).collect();

    // Robust refit: drop listings far from the first fit (lowballs, wild asks) and refit on the
    // rest, reusing the same ranges/pairs (computed once, from every row).
    let centre = finish::median(&residuals);
    let spread_mad = finish::mad(&residuals);
    if spread_mad > 0.0 {
        let threshold = OUTLIER_MADS * MAD_TO_SIGMA * spread_mad;
        let keep: Vec<bool> = residuals.iter().map(|r| (r - centre).abs() <= threshold).collect();
        if keep.iter().filter(|k| **k).count() >= (rows_used.len() / 2).max(1) {
            rows_used = rows_used.into_iter().zip(keep).filter(|(_, k)| *k).map(|(r, _)| r).collect();
            let refit_design = design::build_design(&rows_used, &ranges, &group_ranges, &pairs);
            names = refit_design.names;
            targets = rows_used.iter().map(|r| r.price.ln()).collect();
            fit = ridge::fit_ridge(&refit_design.matrix, &targets, opts.alpha);
            residuals = targets.iter().zip(ridge::predict(&refit_design.matrix, &fit)).map(|(t, p)| t - p).collect();
        }
    }

    let mut coef: HashMap<String, f64> =
        names.into_iter().zip(fit.coef).filter(|(_, c)| c.abs() > COEF_EPSILON).collect();
    let residuals = finish::calibrate_items(&mut coef, &rows_used, &residuals);

    let slots: HashMap<String, String> =
        rows_used.iter().filter(|r| r.slot != UNKNOWN_SLOT).map(|r| (r.item_id.clone(), r.slot.clone())).collect();
    let mut support: HashMap<String, u64> = HashMap::new();
    for row in &rows_used {
        *support.entry(row.item_id.clone()).or_insert(0) += 1;
    }
    let medians = finish::unrolled_medians(&rows_used);
    let finished = finish::finish(&rows_used, &residuals, &coef, &ranges, &group_ranges);

    Ok(TrainedModel {
        intercept: fit.intercept,
        coef,
        ranges,
        group_ranges,
        pairs,
        slots,
        support,
        listings: rows_used.len() as u64,
        medians,
        avg_roll: finished.avg_roll,
        spread: finished.spread,
        group_spread: finished.group_spread,
        global_spread: finished.global_spread,
        low_offset: finished.low_offset,
        group_low_offset: finished.group_low_offset,
        global_low_offset: finished.global_low_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn train_rejects_a_listing_set_with_nothing_priced() {
        let listings = vec![listing("Helm_5001", 0, &[])];
        assert!(matches!(train(&listings, &HashMap::new(), TrainOptions::default()), Err(TrainError::NoListings)));
    }

    #[test]
    fn train_produces_json_worth_model_from_json_accepts() {
        let mut listings = Vec::new();
        for i in 0..60 {
            let strength = 1.0 + (i % 3) as f64;
            listings.push(listing("Helm_5001", (500.0 * strength) as i64, &[("Strength", strength)]));
        }
        let json = train(&listings, &HashMap::new(), TrainOptions::default()).unwrap().to_json();
        assert_eq!(json["version"], 1);
        let model = crate::worth::WorthModel::from_json(&serde_json::to_string(&json).unwrap()).unwrap();
        assert!(model.knows("Helm_5001"));
        let weak = model.predict("Helm_5001", &[("Strength".to_string(), 1.0)], 1, None, None);
        let strong = model.predict("Helm_5001", &[("Strength".to_string(), 3.0)], 1, None, None);
        assert!(strong.value > weak.value);
    }

    #[test]
    fn predict_value_agrees_with_worth_model_predict_at_the_fresh_age_bucket() {
        let mut listings = Vec::new();
        for i in 0..80 {
            let strength = 1.0 + (i % 3) as f64;
            listings.push(listing("Helm_5001", (500.0 * strength) as i64, &[("Strength", strength)]));
        }
        let trained = train(&listings, &HashMap::new(), TrainOptions::default()).unwrap();
        let json = trained.to_json();
        let model = crate::worth::WorthModel::from_json(&serde_json::to_string(&json).unwrap()).unwrap();
        let rolls = vec![("Strength".to_string(), 2.0)];
        let via_predict = model.predict("Helm_5001", &rolls, 1, None, None).value;
        let via_predict_value = trained.predict_value("Helm_5001", &rolls, None, None);
        assert!((via_predict - via_predict_value).abs() / via_predict < 1e-9);
    }

    #[test]
    fn robust_refit_drops_a_lowball_that_would_otherwise_drag_the_price_down() {
        let mut listings = Vec::new();
        for _ in 0..60 {
            listings.push(listing("Helm_5001", 1000, &[("Strength", 2.0)]));
        }
        for _ in 0..8 {
            listings.push(listing("Helm_5001", 20, &[("Strength", 2.0)])); // wild lowballs
        }
        let json = train(&listings, &HashMap::new(), TrainOptions::default()).unwrap().to_json();
        let model = crate::worth::WorthModel::from_json(&serde_json::to_string(&json).unwrap()).unwrap();
        let value = model.predict("Helm_5001", &[("Strength".to_string(), 2.0)], 1, None, None).value;
        assert!((value - 1000.0).abs() / 1000.0 < 0.1, "value {value} was dragged toward the lowballs");
    }
}
