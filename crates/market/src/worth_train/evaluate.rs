//! Holdout evaluation: port of `worth_model.py`'s `evaluate`. Trains on a share of the listings
//! and compares its error on the rest against a plain item-median guess.

use std::collections::HashMap;

use super::{finish, train, TrainError, TrainListing, TrainOptions};

const DEFAULT_HOLDOUT: f64 = 0.2;
const DEFAULT_SEED: u64 = 7;
const WITHIN_RATIO: f64 = 0.25;

/// Holdout accuracy: the model's error against a plain per-item median guess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvalReport {
    pub tested: usize,
    pub trained: usize,
    /// Median absolute percentage error of the model's guesses.
    pub model_mdape: f64,
    /// Share of the model's guesses within 25% of the actual price.
    pub model_within_25: f64,
    pub baseline_mdape: f64,
    pub baseline_within_25: f64,
}

/// A splitmix64 generator: small, seedable, and good enough for an evaluation shuffle (this need
/// not reproduce Python's Mersenne-Twister shuffle bit for bit — each language's `evaluate` is
/// judged against its own baseline, not against the other's exact holdout split).
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A uniform index in `0..bound` (`bound > 0`); the modulo bias is negligible at our list
    /// sizes and this only decides an evaluation split, not the model itself.
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// An in-place Fisher-Yates shuffle seeded by `seed`.
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut rng = SplitMix64(seed);
    for i in (1..items.len()).rev() {
        let j = rng.below(i + 1);
        items.swap(i, j);
    }
}

/// Holds out `holdout` of `listings`, trains on the rest, and compares errors with an item-median
/// guess. `holdout`/`seed` default to `worth_model.py`'s `evaluate` defaults (0.2, 7).
pub fn evaluate(
    listings: &[TrainListing],
    item_slots: &HashMap<String, String>,
    holdout: f64,
    seed: u64,
    opts: TrainOptions,
) -> Result<EvalReport, TrainError> {
    let mut shuffled: Vec<&TrainListing> = listings.iter().collect();
    shuffle(&mut shuffled, seed);
    let cut = (((shuffled.len() as f64) * holdout) as usize).clamp(1, shuffled.len().max(1));
    let (tested, trained) = shuffled.split_at(cut.min(shuffled.len()));
    let trained_owned: Vec<TrainListing> = trained.iter().map(|l| (*l).clone()).collect();
    let model = train(&trained_owned, item_slots, opts)?;

    let mut unit: HashMap<&str, Vec<f64>> = HashMap::new();
    for listing in trained {
        if listing.price > 0 {
            unit.entry(listing.item_id.as_str()).or_default().push(listing.price as f64 / listing.item_count.max(1) as f64);
        }
    }
    let everything: Vec<f64> = unit.values().flat_map(|v| v.iter().copied()).collect();
    let fallback = finish::median(&everything);

    let mut model_err = Vec::new();
    let mut base_err = Vec::new();
    for listing in tested {
        if listing.price <= 0 {
            continue;
        }
        let actual = listing.price as f64 / listing.item_count.max(1) as f64;
        let slot = item_slots.get(&listing.item_id).map(String::as_str);
        let guess = model.predict_value(&listing.item_id, &listing.rolls, slot, listing.age_days);
        let base = unit.get(listing.item_id.as_str()).map_or(fallback, |v| finish::median(v));
        model_err.push((guess - actual).abs() / actual);
        base_err.push((base - actual).abs() / actual);
    }

    let summary = |errors: &[f64]| -> (f64, f64) {
        let mdape = finish::median(errors) * 100.0;
        let within = errors.iter().filter(|&&e| e <= WITHIN_RATIO).count() as f64 / errors.len() as f64 * 100.0;
        ((mdape * 10.0).round() / 10.0, (within * 10.0).round() / 10.0)
    };
    let (model_mdape, model_within_25) = summary(&model_err);
    let (baseline_mdape, baseline_within_25) = summary(&base_err);
    Ok(EvalReport { tested: model_err.len(), trained: trained.len(), model_mdape, model_within_25, baseline_mdape, baseline_within_25 })
}

/// [`evaluate`] with `worth_model.py`'s defaults (`holdout=0.2, seed=7`).
pub fn evaluate_default(
    listings: &[TrainListing],
    item_slots: &HashMap<String, String>,
    opts: TrainOptions,
) -> Result<EvalReport, TrainError> {
    evaluate(listings, item_slots, DEFAULT_HOLDOUT, DEFAULT_SEED, opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_is_deterministic_for_a_given_seed_and_touches_every_element() {
        let mut a: Vec<i32> = (0..20).collect();
        let mut b = a.clone();
        shuffle(&mut a, 42);
        shuffle(&mut b, 42);
        assert_eq!(a, b);
        assert_ne!(a, (0..20).collect::<Vec<_>>()); // vanishingly unlikely to shuffle to itself
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..20).collect::<Vec<_>>());
    }

    fn listing(item_id: &str, price: i64) -> TrainListing {
        TrainListing { item_id: item_id.to_owned(), rarity: 5, price, item_count: 1, rolls: vec![], age_days: None }
    }

    #[test]
    fn evaluate_needs_no_more_than_the_holdout_and_reports_matching_counts() {
        let listings: Vec<TrainListing> = (0..50).map(|i| listing("Coin", 100 + i)).collect();
        let report = evaluate(&listings, &HashMap::new(), DEFAULT_HOLDOUT, DEFAULT_SEED, TrainOptions::default()).unwrap();
        assert_eq!(report.trained, 40);
        assert!(report.tested <= 10);
        assert!(report.model_mdape >= 0.0 && report.baseline_mdape >= 0.0);
    }

    #[test]
    fn evaluate_fails_the_same_way_train_does_on_no_usable_listings() {
        let listings = vec![listing("Coin", 0)];
        assert_eq!(
            evaluate(&listings, &HashMap::new(), DEFAULT_HOLDOUT, DEFAULT_SEED, TrainOptions::default()),
            Err(TrainError::NoListings)
        );
    }
}
