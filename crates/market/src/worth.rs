//! Item Worth: what an item is worth for its exact rolls, from the model trained on the market
//! history (DnDTools' `worth_model.json`, version 1). Prediction only; see the Python
//! `worth_model.py` for how it is trained.
//!
//! log(price) = group(slot, rarity) + item
//!              + Σ rolls [ e(stat, tier) + e(stat, rarity, tier) + e(stat, slot, rarity, tier) ]
//!              + Σ pairs e(stat A + stat B)

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::history::rarity_of;

const MODEL_VERSION: u32 = 1;
const TIER_EDGES: (f64, f64) = (1.0 / 3.0, 2.0 / 3.0);
const MAD_TO_SIGMA: f64 = 1.4826;
const HIGH_CONFIDENCE_SUPPORT: u64 = 30;
const MEDIUM_CONFIDENCE_SUPPORT: u64 = 8;
/// A typical log error below this counts as a tight fit.
const HIGH_CONFIDENCE_SPREAD: f64 = 0.35;
const UNKNOWN_SLOT: &str = "?";
/// Pair bonuses smaller than this count in the value but aren't listed.
const PAIR_REPORT_PCT: f64 = 1.0;
/// Listings are predicted as fresh (the age bucket of a listing just put up).
const FRESH: u32 = 0;
/// Typical low end vs value across the market (for models saved without their own).
fn default_low_offset() -> f64 {
    0.81f64.ln()
}
fn default_global_spread() -> f64 {
    0.3
}

#[derive(Debug, thiserror::Error)]
pub enum WorthError {
    #[error("not a saved worth model: {0}")]
    Json(#[from] serde_json::Error),
    #[error("worth model version {0} is not supported (expected {MODEL_VERSION})")]
    Version(u32),
    #[error("could not read the worth model: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
    /// Neither the item nor anything like it (its slot and rarity) was ever listed.
    Unknown,
}

/// What one roll does to the price.
#[derive(Debug, Clone, PartialEq)]
pub struct RollWorth {
    pub stat: String,
    pub value: f64,
    /// Where the roll sits in the stat's range on this item, 0 (worst) .. 1 (best).
    pub quality: f64,
    /// Price change against an average roll, in percent.
    pub effect_pct: f64,
}

/// A pair of stats that sells for more (or less) together.
#[derive(Debug, Clone, PartialEq)]
pub struct PairWorth {
    pub stats: Vec<String>,
    pub effect_pct: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    /// The typical ask for these exact rolls, for the whole quantity.
    pub value: f64,
    /// Lowest reasonable price: only ~20% of real asks for these rolls sit below it.
    pub floor: f64,
    pub low: f64,
    pub high: f64,
    pub confidence: Confidence,
    /// The same item with average rolls.
    pub typical: f64,
    pub rolls: Vec<RollWorth>,
    pub pairs: Vec<PairWorth>,
    /// Listings of this item the model learned from.
    pub listings: u64,
}

type Ranges = HashMap<String, HashMap<String, [f64; 2]>>;

#[derive(Debug, Deserialize)]
struct Data {
    version: u32,
    intercept: f64,
    #[serde(default)]
    coef: HashMap<String, f64>,
    #[serde(default)]
    ranges: Ranges,
    #[serde(default)]
    group_ranges: Ranges,
    #[serde(default)]
    pairs: Vec<String>,
    #[serde(default)]
    slots: HashMap<String, String>,
    #[serde(default)]
    support: HashMap<String, u64>,
    #[serde(default)]
    listings: u64,
    #[serde(default)]
    medians: HashMap<String, f64>,
    #[serde(default)]
    avg_roll: HashMap<String, f64>,
    #[serde(default)]
    spread: HashMap<String, f64>,
    #[serde(default)]
    group_spread: HashMap<String, f64>,
    #[serde(default = "default_global_spread")]
    global_spread: f64,
    #[serde(default)]
    low_offset: HashMap<String, f64>,
    #[serde(default)]
    group_low_offset: HashMap<String, f64>,
    #[serde(default = "default_low_offset")]
    global_low_offset: f64,
}

/// A trained worth model.
#[derive(Debug)]
pub struct WorthModel {
    d: Data,
    /// Each saved pair ("ActionSpeed+Luck") split into its stats.
    pairs: Vec<(String, Vec<String>)>,
}

impl WorthModel {
    pub fn load(path: &Path) -> Result<Self, WorthError> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn from_json(json: &str) -> Result<Self, WorthError> {
        let d: Data = serde_json::from_str(json)?;
        if d.version != MODEL_VERSION {
            return Err(WorthError::Version(d.version));
        }
        let pairs = d.pairs.iter().map(|p| (p.clone(), p.split('+').map(str::to_owned).collect())).collect();
        Ok(Self { d, pairs })
    }

    /// Listings the model was trained on.
    pub fn listings(&self) -> u64 {
        self.d.listings
    }

    /// Whether the model saw listings of this item.
    pub fn knows(&self, item_id: &str) -> bool {
        self.d.support.contains_key(item_id)
    }

    fn slot<'a>(&'a self, item_id: &str, slot: Option<&'a str>) -> &'a str {
        self.d.slots.get(item_id).map(String::as_str).or(slot).filter(|s| !s.is_empty()).unwrap_or(UNKNOWN_SLOT)
    }

    fn range(&self, item_id: &str, slot: &str, rarity: i64, stat: &str) -> Option<[f64; 2]> {
        self.d
            .ranges
            .get(item_id)
            .and_then(|stats| stats.get(stat))
            .or_else(|| self.d.group_ranges.get(&format!("{slot}|{rarity}")).and_then(|stats| stats.get(stat)))
            .copied()
    }

    /// Where a roll sits in the stat's range on this item: 0 (worst) .. 1 (best), 0.5 when unknown.
    pub fn quality(&self, item_id: &str, stat: &str, value: f64, slot: Option<&str>, rarity: Option<i64>) -> f64 {
        let slot = self.slot(item_id, slot);
        let rarity = rarity.unwrap_or_else(|| rarity_of(item_id));
        self.range(item_id, slot, rarity, stat).map_or(0.5, |[low, high]| roll_quality(value, low, high))
    }

    fn coef(&self, name: &str) -> f64 {
        self.d.coef.get(name).copied().unwrap_or(0.0)
    }

    fn roll_effect(&self, stat: &str, slot: &str, rarity: i64, quality: f64) -> f64 {
        let tier = tier(quality);
        self.coef(&format!("r|{stat}|{tier}"))
            + self.coef(&format!("rr|{stat}|{rarity}|{tier}"))
            + self.coef(&format!("rs|{stat}|{slot}|{rarity}|{tier}"))
            + self.coef(&format!("q|{stat}|{rarity}")) * (quality - 0.5)
    }

    fn low_offset(&self, item_id: &str, group: &str) -> f64 {
        self.d
            .low_offset
            .get(item_id)
            .or_else(|| self.d.group_low_offset.get(group))
            .copied()
            .unwrap_or(self.d.global_low_offset)
    }

    /// What `quantity` of this item with these rolls sells for as a fresh listing. `slot` (the
    /// catalog's slot type) and `rarity` are used when the model has not seen the item.
    pub fn predict(
        &self,
        item_id: &str,
        rolls: &[(String, f64)],
        quantity: u32,
        slot: Option<&str>,
        rarity: Option<i64>,
    ) -> Estimate {
        let rarity = rarity.unwrap_or_else(|| rarity_of(item_id));
        let slot = self.slot(item_id, slot);
        let group = format!("{slot}|{rarity}");
        let mut log_base = self.d.intercept
            + self.coef(&format!("g|{group}"))
            + self.coef(&format!("i|{item_id}"))
            + self.coef(&format!("a|{FRESH}"));
        if rolls.is_empty() {
            // Unrolled items: the median ask beats any fit.
            if let Some(median) = self.d.medians.get(item_id).filter(|m| **m > 0.0) {
                log_base = median.ln();
            }
        }
        let average = self.d.avg_roll.get(&rarity.to_string()).copied().unwrap_or(0.0);
        let roll_parts: Vec<(&str, f64, f64, f64)> = rolls
            .iter()
            .map(|(stat, value)| {
                let quality = self.range(item_id, slot, rarity, stat).map_or(0.5, |[l, h]| roll_quality(*value, l, h));
                (stat.as_str(), *value, quality, self.roll_effect(stat, slot, rarity, quality))
            })
            .collect();
        let stats: HashSet<&str> = rolls.iter().map(|(stat, _)| stat.as_str()).collect();
        let pair_parts: Vec<(&Vec<String>, f64)> = self
            .pairs
            .iter()
            .filter(|(_, parts)| parts.iter().all(|p| stats.contains(p.as_str())))
            .map(|(key, parts)| (parts, self.coef(&format!("p|{key}"))))
            .collect();
        let log_value = log_base
            + roll_parts.iter().map(|(.., effect)| effect).sum::<f64>()
            + pair_parts.iter().map(|(_, effect)| effect).sum::<f64>();
        let quantity = f64::from(quantity.max(1));
        let value = log_value.exp() * quantity;
        // A saved spread of 0 means "not measured", as in the Python model.
        let spread = [self.d.spread.get(item_id), self.d.group_spread.get(&group)]
            .into_iter()
            .flatten()
            .copied()
            .find(|s| *s != 0.0)
            .unwrap_or(self.d.global_spread);
        let band = (MAD_TO_SIGMA * spread).exp();
        let support = self.d.support.get(item_id).copied().unwrap_or(0);
        let confidence = if support == 0 && !self.d.coef.contains_key(&format!("g|{group}")) {
            Confidence::Unknown
        } else if support >= HIGH_CONFIDENCE_SUPPORT && spread <= HIGH_CONFIDENCE_SPREAD {
            Confidence::High
        } else if support >= MEDIUM_CONFIDENCE_SUPPORT {
            Confidence::Medium
        } else {
            Confidence::Low
        };
        Estimate {
            value,
            floor: value.min(value * self.low_offset(item_id, &group).exp()),
            low: value / band,
            high: value * band,
            confidence,
            typical: (log_base + average * roll_parts.len() as f64).exp() * quantity,
            rolls: roll_parts
                .iter()
                .map(|&(stat, value, quality, effect)| RollWorth {
                    stat: stat.to_owned(),
                    value,
                    quality,
                    effect_pct: ((effect - average).exp() - 1.0) * 100.0,
                })
                .collect(),
            pairs: pair_parts
                .into_iter()
                .map(|(parts, effect)| PairWorth { stats: parts.clone(), effect_pct: (effect.exp() - 1.0) * 100.0 })
                .filter(|pair| round1(pair.effect_pct).abs() >= PAIR_REPORT_PCT)
                .collect(),
            listings: support,
        }
    }
}

/// Where a roll sits in its range, 0 (worst) .. 1 (best); 0.5 when the range is unknown.
pub fn roll_quality(value: f64, low: f64, high: f64) -> f64 {
    if high <= low {
        0.5
    } else {
        ((value - low) / (high - low)).clamp(0.0, 1.0)
    }
}

fn tier(quality: f64) -> u8 {
    if quality < TIER_EDGES.0 {
        0
    } else if quality < TIER_EDGES.1 {
        1
    } else {
        2
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::{roll_quality, tier};

    #[test]
    fn quality_is_the_place_in_the_range() {
        assert_eq!(roll_quality(20.0, 10.0, 30.0), 0.5);
        assert_eq!(roll_quality(40.0, 10.0, 30.0), 1.0);
        assert_eq!(roll_quality(5.0, 10.0, 30.0), 0.0);
        assert_eq!(roll_quality(5.0, 5.0, 5.0), 0.5);
    }

    #[test]
    fn tiers_are_thirds() {
        assert_eq!((tier(0.0), tier(0.33), tier(0.34), tier(0.66), tier(0.67), tier(1.0)), (0, 0, 1, 1, 2, 2));
    }
}
