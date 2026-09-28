//! Port of `test_market_patterns.py` and `test_market_model.py`.

use std::collections::HashMap;

use market::patterns::Listing;
use market::{
    analyze, extra_good_roll_factor, extra_roll_share, good_roll_counts, load_model, model_from_report,
    pair_bonuses, percentile, rarity_steps, roll_ranges, save_model, stat_premiums, Report,
};

/// A small xorshift64 generator: deterministic and dependency-free. It need not reproduce
/// Python's Mersenne-Twister draw for draw — each language's test checks its own generated market
/// against generous tolerances, not the other's exact numbers.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in `[lo, hi]` inclusive.
    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.unit() * (hi - lo + 1) as f64) as i64
    }

    fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.int(0, items.len() as i64 - 1) as usize]
    }
}

fn listing(item_id: &str, rarity: i64, price: i64, rolls: &[(&str, f64)]) -> Listing {
    Listing {
        item_id: item_id.to_owned(),
        rarity,
        price,
        item_count: 1,
        base: vec![],
        rolls: rolls.iter().map(|(s, v)| ((*s).to_owned(), *v)).collect(),
        seller: String::new(),
    }
}

/// Synthetic shields: Luck is valuable (+60% at best roll), Vigor worthless; each good roll adds
/// value. Port of `test_market_patterns.py`'s `_market`.
fn market(seed: u64) -> Vec<Listing> {
    let mut rng = Rng::new(seed);
    let mut rows = Vec::new();
    for item in ["ShieldA_5001", "ShieldB_5001", "ShieldC_5001"] {
        let base_price = *rng.choice(&[200.0, 400.0, 800.0]);
        for _ in 0..40 {
            let luck = rng.int(10, 20);
            let vigor = rng.int(1, 5);
            let power = rng.int(1, 4);
            let (q_luck, q_power) = ((luck - 10) as f64 / 10.0, (power - 1) as f64 / 3.0);
            let noise = 0.95 + rng.unit() * 0.10;
            let price = base_price * (1.0 + 0.6 * q_luck) * (1.0 + 0.3 * q_power) * noise;
            rows.push(listing(item, 5, price as i64, &[("Luck", luck as f64), ("Vigor", vigor as f64), ("Power", power as f64)]));
        }
    }
    rows
}

#[test]
fn roll_ranges_and_percentile() {
    let listings = vec![listing("A_5001", 5, 100, &[("Luck", 10.0)]), listing("A_5001", 5, 100, &[("Luck", 20.0)])];
    let ranges = roll_ranges(&listings);
    assert_eq!(ranges["A_5001"]["Luck"], (10.0, 20.0, 2));
    assert_eq!(percentile(17.0, 10.0, 20.0), 0.7);
    assert_eq!(percentile(5.0, 5.0, 5.0), 1.0);
}

#[test]
fn stat_premiums_find_the_valuable_stat() {
    let premiums = stat_premiums(&market(7));
    let by_stat: HashMap<&str, _> = premiums.iter().map(|(s, p)| (s.as_str(), *p)).collect();
    assert!(by_stat["Luck"].per_quality > 40.0); // planted +60%
    assert!(by_stat["Vigor"].per_quality.abs() < 10.0); // planted 0
    assert_eq!(premiums[0].0, "Luck"); // sorted most valuable first
}

#[test]
fn more_good_rolls_are_worth_more() {
    let counts = good_roll_counts(&market(7));
    let mut keys: Vec<u32> = counts.iter().filter(|(_, c)| c.support >= 8).map(|(&k, _)| k).collect();
    keys.sort_unstable();
    let uplifts: Vec<f64> = keys.iter().map(|k| counts[k].median_uplift).collect();
    let mut sorted = uplifts.clone();
    sorted.sort_by(f64::total_cmp);
    assert_eq!(uplifts, sorted);
    let factor = extra_good_roll_factor(&counts);
    assert!(factor.is_some_and(|f| f > 0.0 && f <= 1.0));
}

#[test]
fn rarity_steps_between_consecutive_rarities() {
    let listings = vec![
        listing("Axe_4001", 4, 100, &[]),
        listing("Axe_4001", 4, 110, &[]),
        listing("Axe_4001", 4, 90, &[]),
        listing("Axe_5001", 5, 300, &[]),
        listing("Axe_5001", 5, 310, &[]),
        listing("Axe_5001", 5, 290, &[]),
    ];
    assert_eq!(rarity_steps(&listings)["4->5"].median_ratio, 3.0);
}

#[test]
fn analyze_runs_end_to_end() {
    let listings = market(7);
    let report = analyze(&listings, &HashMap::from([("ShieldA_5001".to_string(), "shield".to_string())]));
    assert_eq!(report.listings, 120);
    assert_eq!(report.items, 3);
    assert!(!report.pair_synergies.is_empty());
    assert!(report.lowball_share.ends_with('%'));
}

// --- market_model.py ---

fn pair_synergy_entries() -> serde_json::Value {
    serde_json::json!([
        {"pair": "PhysicalPower + PhysicalWeaponDamageAdd", "synergy": 18.0, "support": 33},
        {"pair": "Agility + Knowledge", "synergy": 14.0, "support": 9},
        {"pair": "Luck + Vigor", "synergy": -6.0, "support": 40},
        {"pair": "broken", "synergy": 5.0, "support": 50},
    ])
}

#[test]
fn model_keeps_what_pricing_needs() {
    let report = Report { extra_good_roll_factor: Some(0.2), listings: 10, ..Default::default() };
    let model = model_from_report(&report);
    let keys: std::collections::BTreeSet<&str> = model.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from(["stat_premiums", "good_roll_counts", "extra_good_roll_factor", "roll_ranges", "pair_synergies"])
    );
}

#[test]
fn save_replaces_the_model_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("market_model.json");
    save_model(&path, &serde_json::json!({"extra_good_roll_factor": 0.1})).unwrap();
    save_model(&path, &serde_json::json!({"extra_good_roll_factor": 0.3})).unwrap();
    assert_eq!(load_model(&path), serde_json::json!({"extra_good_roll_factor": 0.3}));
    let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(entries.len(), 1); // no temporary files left behind
}

#[test]
fn missing_or_broken_model_loads_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_model(&dir.path().join("nope.json")), serde_json::json!({}));
    let bad = dir.path().join("bad.json");
    std::fs::write(&bad, "{half a fi").unwrap();
    assert_eq!(load_model(&bad), serde_json::json!({}));
    let list = dir.path().join("list.json");
    std::fs::write(&list, "[1, 2]").unwrap();
    assert_eq!(load_model(&list), serde_json::json!({}));
}

#[test]
fn pair_bonuses_keep_only_positive_well_supported_pairs() {
    let model = serde_json::json!({"pair_synergies": pair_synergy_entries()});
    let bonuses = pair_bonuses(&model, 10);
    assert_eq!(bonuses.len(), 1);
    let key = std::collections::BTreeSet::from(["PhysicalPower".to_string(), "PhysicalWeaponDamageAdd".to_string()]);
    assert_eq!(bonuses.get(&key), Some(&18.0));
    assert!(pair_bonuses(&serde_json::json!({}), 10).is_empty());
}

#[test]
fn extra_roll_share_is_clamped_with_a_default() {
    assert_eq!(extra_roll_share(&serde_json::json!({"extra_good_roll_factor": 0.4}), 0.25), 0.4);
    assert_eq!(extra_roll_share(&serde_json::json!({"extra_good_roll_factor": 7}), 0.25), 1.0);
    assert_eq!(extra_roll_share(&serde_json::json!({"extra_good_roll_factor": -1}), 0.25), 0.0);
    assert_eq!(extra_roll_share(&serde_json::json!({"extra_good_roll_factor": "x"}), 0.25), 0.25);
    assert_eq!(extra_roll_share(&serde_json::json!({}), 0.25), 0.25);
}
