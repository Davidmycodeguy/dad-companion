//! Port of `test_worth_model.py`.
//!
//! The synthetic markets below mirror the Python fixtures' *shape* (which items, which planted
//! effects) but draw from a different RNG, so exact numbers differ from Python's — tests use the
//! same generous tolerances Python's do, which hold for any reasonable random realization of the
//! same data-generating process, not just one specific seed's draws.

use std::collections::HashMap;

use market::worth_train::Candidate;
use market::{evaluate, roll_quality, similar, train, Estimate, TrainListing, TrainOptions, WorthModel};

const STR_EFFECT: [f64; 4] = [0.0, 1.0, 1.3, 1.6]; // index by roll value 1..3
const JUNK_EFFECT: f64 = 0.8;
const FILLER: [&str; 4] = ["Knowledge", "Will", "Agility", "Luck"];

fn base_price(item: &str) -> f64 {
    match item {
        "Helm_5001" => 500.0,
        "Boots_5001" => 800.0,
        "Cap_5001" => 400.0,
        _ => 0.0,
    }
}

fn item_slots() -> HashMap<String, String> {
    HashMap::from([
        ("Helm_5001".to_string(), "Head".to_string()),
        ("Boots_5001".to_string(), "Foot".to_string()),
        ("Cap_5001".to_string(), "Head".to_string()),
        ("GoldBand_3001".to_string(), "Ring".to_string()),
    ])
}

/// A small xorshift64 generator: deterministic and dependency-free (need not reproduce Python's
/// Mersenne-Twister draw for draw — see the module doc).
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

    /// A standard-normal draw (Box-Muller), scaled to `mean`/`std`.
    fn gauss(&mut self, mean: f64, std: f64) -> f64 {
        let (u1, u2) = (self.unit().max(1e-12), self.unit());
        mean + std * (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

fn rolls(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
    pairs.iter().map(|(s, v)| ((*s).to_string(), *v)).collect()
}

fn listing(item_id: &str, rarity: i64, price: i64, rolls: Vec<(String, f64)>) -> TrainListing {
    TrainListing { item_id: item_id.to_owned(), rarity, price, item_count: 1, rolls, age_days: None }
}

const MARKET_ITEMS: [&str; 3] = ["Helm_5001", "Boots_5001", "Cap_5001"];

/// Port of `test_worth_model.py`'s `_market`: price = base x Strength effect x junk effect x
/// (Dex+Vigor pair) x noise.
fn market(seed: u64, n: usize, pair_bonus: f64, lowballs: usize) -> Vec<TrainListing> {
    let mut rng = Rng::new(seed);
    let mut rows = Vec::new();
    for _ in 0..n {
        let item = *rng.choice(&MARKET_ITEMS);
        let strength = rng.int(1, 3);
        let mut roll_list = vec![("Strength".to_string(), strength as f64)];
        let mut price = base_price(item) * STR_EFFECT[strength as usize];
        if rng.unit() < 0.4 {
            let value = rng.int(20, 40) as f64;
            roll_list.push(("UndeadDamageMod".to_string(), value));
            price *= JUNK_EFFECT;
        } else {
            roll_list.push((rng.choice(&FILLER).to_string(), rng.int(1, 3) as f64));
        }
        if rng.unit() < 0.35 {
            let last = roll_list.len() - 1;
            roll_list[last] = ("Dexterity".to_string(), 2.0);
            roll_list.push(("Vigor".to_string(), 2.0));
            price *= pair_bonus;
        } else {
            roll_list.push((rng.choice(&FILLER).to_string(), rng.int(1, 3) as f64));
        }
        price *= (rng.gauss(0.0, 0.08)).exp();
        rows.push(listing(item, 5, (price as i64).max(1), roll_list));
    }
    for _ in 0..lowballs {
        rows.push(listing("Helm_5001", 5, 40, rolls(&[("Strength", 3.0), ("Luck", 2.0), ("Will", 1.0)])));
    }
    rows
}

/// Port of `test_worth_model.py`'s `_pair_market`: Dexterity and Vigor also appear apart, so a
/// bonus for having both is identifiable.
fn pair_market(seed: u64, n: usize, bonus: f64) -> Vec<TrainListing> {
    let mut rng = Rng::new(seed);
    let mut rows = Vec::new();
    for _ in 0..n {
        let item = *rng.choice(&MARKET_ITEMS);
        let kind = rng.unit();
        let extra = if kind < 0.3 {
            vec![("Dexterity".to_string(), 2.0), ("Vigor".to_string(), 2.0)]
        } else if kind < 0.5 {
            vec![("Dexterity".to_string(), 2.0), (rng.choice(&FILLER).to_string(), 2.0)]
        } else if kind < 0.7 {
            vec![("Vigor".to_string(), 2.0), (rng.choice(&FILLER).to_string(), 2.0)]
        } else {
            vec![(rng.choice(&FILLER).to_string(), 1.0), (rng.choice(&FILLER[..2]).to_string(), 3.0)]
        };
        let price = base_price(item) * (if kind < 0.3 { bonus } else { 1.0 }) * rng.gauss(0.0, 0.08).exp();
        let mut roll_list = vec![("Strength".to_string(), 2.0)];
        roll_list.extend(extra);
        rows.push(listing(item, 5, price as i64, roll_list));
    }
    rows
}

/// Trains and loads a `WorthModel`, mirroring `test_worth_model.py`'s `_train` (`min_pair_support`
/// defaults to 30 there, not `worth_model.py`'s own default of 50).
fn train_model(listings: &[TrainListing]) -> WorthModel {
    train_model_with(listings, 30)
}

fn train_model_with(listings: &[TrainListing], min_pair_support: usize) -> WorthModel {
    let json = train(listings, &item_slots(), TrainOptions { min_pair_support, alpha: 1.0 }).unwrap().to_json();
    WorthModel::from_json(&serde_json::to_string(&json).unwrap()).unwrap()
}

fn effect_pct(est: &Estimate, stat: &str) -> f64 {
    est.rolls.iter().find(|r| r.stat == stat).unwrap_or_else(|| panic!("no {stat} roll in {est:?}")).effect_pct
}

#[test]
fn strong_rolls_raise_and_junk_rolls_lower_the_prediction() {
    let model = train_model(&market(1, 320, 1.0, 0));
    let weak = model.predict("Helm_5001", &rolls(&[("Strength", 1.0), ("Luck", 2.0), ("Will", 2.0)]), 1, None, None);
    let strong = model.predict("Helm_5001", &rolls(&[("Strength", 3.0), ("Luck", 2.0), ("Will", 2.0)]), 1, None, None);
    let junk = model.predict("Helm_5001", &rolls(&[("Strength", 3.0), ("UndeadDamageMod", 30.0), ("Will", 2.0)]), 1, None, None);
    assert!((strong.value / weak.value - 1.6).abs() < 0.15);
    assert!((junk.value / strong.value - JUNK_EFFECT).abs() < 0.1);
    assert!((strong.value - 500.0 * 1.6).abs() / (500.0 * 1.6) < 0.15);
    assert!(effect_pct(&junk, "Strength") > 0.0);
    assert!(0.0 > effect_pct(&junk, "UndeadDamageMod"));
}

#[test]
fn stat_pairs_that_sell_together_are_worth_more() {
    let model = train_model(&pair_market(5, 800, 1.3));
    let both = model.predict("Boots_5001", &rolls(&[("Strength", 2.0), ("Dexterity", 2.0), ("Vigor", 2.0)]), 1, None, None);
    let dex = model.predict("Boots_5001", &rolls(&[("Strength", 2.0), ("Dexterity", 2.0), ("Luck", 2.0)]), 1, None, None);
    let vig = model.predict("Boots_5001", &rolls(&[("Strength", 2.0), ("Vigor", 2.0), ("Luck", 2.0)]), 1, None, None);
    assert!((both.value / dex.value - 1.3).abs() < 0.1);
    assert!((both.value / vig.value - 1.3).abs() < 0.1);
    let best_pair = both.pairs.iter().max_by(|a, b| a.effect_pct.total_cmp(&b.effect_pct)).expect("a pair bonus was learned");
    assert_eq!(best_pair.stats, vec!["Dexterity".to_string(), "Vigor".to_string()]);
}

#[test]
fn unseen_items_fall_back_to_their_slot_and_rarity() {
    let model = train_model(&market(1, 320, 1.0, 0));
    let guess = model.predict("NewHelm_5001", &rolls(&[("Strength", 2.0), ("Luck", 2.0), ("Will", 2.0)]), 1, Some("Head"), None);
    assert_eq!(guess.confidence, market::Confidence::Low);
    assert!(300.0 < guess.value && guess.value < 1000.0);
}

#[test]
fn unrolled_items_are_priced_per_unit() {
    let mut listings = market(1, 320, 1.0, 0);
    for (price, count) in [(100, 1), (110, 1), (95, 1), (300, 3), (105, 1), (98, 1), (102, 1), (97, 1)] {
        listings.push(TrainListing { item_id: "GoldBand_3001".to_string(), rarity: 3, price, item_count: count, rolls: vec![], age_days: None });
    }
    let model = train_model(&listings);
    assert!((model.predict("GoldBand_3001", &[], 1, None, None).value - 100.0).abs() < 10.0);
    assert!((model.predict("GoldBand_3001", &[], 3, None, None).value - 300.0).abs() < 30.0);
}

#[test]
fn lowball_outliers_do_not_drag_the_prediction_down() {
    let model = train_model(&market(1, 320, 1.0, 12));
    let value = model.predict("Helm_5001", &rolls(&[("Strength", 3.0), ("Luck", 2.0), ("Will", 1.0)]), 1, None, None).value;
    assert!((value - 800.0).abs() / 800.0 < 0.15);
}

#[test]
fn model_survives_a_json_round_trip_through_a_file() {
    let listings = market(1, 320, 1.0, 0);
    let text = serde_json::to_string(&train(&listings, &item_slots(), TrainOptions { min_pair_support: 30, alpha: 1.0 }).unwrap().to_json()).unwrap();
    let direct = WorthModel::from_json(&text).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worth_model.json");
    std::fs::write(&path, &text).unwrap();
    let loaded = WorthModel::load(&path).unwrap();
    let test_rolls = rolls(&[("Strength", 2.0), ("UndeadDamageMod", 25.0), ("Knowledge", 3.0)]);
    let a = direct.predict("Cap_5001", &test_rolls, 1, None, None).value;
    let b = loaded.predict("Cap_5001", &test_rolls, 1, None, None).value;
    assert!((a - b).abs() < 1e-6);
}

#[test]
fn explanation_adds_up_to_the_value() {
    let model = train_model(&market(1, 320, 1.0, 0));
    let est = model.predict("Helm_5001", &rolls(&[("Strength", 3.0), ("UndeadDamageMod", 30.0), ("Will", 2.0)]), 1, None, None);
    let roll_product: f64 = est.rolls.iter().map(|c| 1.0 + c.effect_pct / 100.0).product();
    let pair_product: f64 = est.pairs.iter().map(|p| 1.0 + p.effect_pct / 100.0).product();
    let total = est.typical * roll_product * pair_product;
    assert!((total - est.value).abs() / est.value < 0.03); // tiny pair effects aren't itemised
    assert!(est.low < est.value && est.value < est.high);
}

#[test]
fn roll_quality_is_measured_within_the_items_range() {
    let model = train_model(&market(1, 320, 1.0, 0));
    let est = model.predict("Helm_5001", &rolls(&[("Strength", 2.0), ("UndeadDamageMod", 40.0), ("Will", 1.0)]), 1, None, None);
    let quality = |stat: &str| est.rolls.iter().find(|r| r.stat == stat).unwrap().quality;
    assert_eq!(quality("Strength"), 0.5); // Strength only ever rolls 1..3, so 2 sits dead center
    assert_eq!(quality("UndeadDamageMod"), 1.0); // rolls 20..40, so 40 is the best seen
    assert_eq!(quality("Will"), 0.0); // filler rolls 1..3, so 1 is the worst seen
    assert_eq!(roll_quality(5.0, 5.0, 5.0), 0.5);
}

#[test]
fn holdout_evaluation_beats_the_item_median_baseline() {
    let listings = market(1, 500, 1.0, 0);
    let report = evaluate(&listings, &item_slots(), 0.25, 3, TrainOptions { min_pair_support: 30, alpha: 1.0 }).unwrap();
    assert!(report.model_mdape < report.baseline_mdape);
    assert!(report.model_within_25 > report.baseline_within_25);
    assert!(report.tested > 100);
}

#[test]
fn similar_listings_share_stats_and_have_close_rolls() {
    let candidates = vec![
        Candidate { item_id: "Helm_5001".to_string(), price: 900.0, rolls: rolls(&[("Strength", 3.0), ("Vigor", 2.0), ("Luck", 1.0)]) },
        Candidate { item_id: "Helm_5001".to_string(), price: 400.0, rolls: rolls(&[("Knowledge", 1.0), ("Will", 1.0), ("Luck", 1.0)]) },
        Candidate { item_id: "Helm_5001".to_string(), price: 700.0, rolls: rolls(&[("Strength", 2.0), ("Vigor", 2.0), ("Will", 1.0)]) },
        Candidate { item_id: "Cap_5001".to_string(), price: 999.0, rolls: rolls(&[("Strength", 3.0), ("Vigor", 2.0), ("Luck", 1.0)]) },
    ];
    let model = train_model(&market(1, 320, 1.0, 0));
    let found = similar(&model, "Helm_5001", &rolls(&[("Strength", 3.0), ("Vigor", 2.0), ("Will", 2.0)]), &candidates, 2);
    assert_eq!(found.iter().map(|c| c.price).collect::<Vec<_>>(), vec![700.0, 900.0]); // same three stats beats two exact matches + a stranger
}

#[test]
fn values_are_for_a_fresh_listing_because_old_listings_are_the_overpriced_ones() {
    let mut rng = Rng::new(9);
    let mut rows = Vec::new();
    for _ in 0..400 {
        let age = if rng.unit() < 0.5 { 0.2 } else { 6.0 };
        let price = 600.0 * (if age > 5.0 { 1.2 } else { 1.0 }) * rng.gauss(0.0, 0.05).exp();
        rows.push(TrainListing {
            item_id: "Helm_5001".to_string(),
            rarity: 5,
            price: price as i64,
            item_count: 1,
            rolls: rolls(&[("Strength", 2.0), ("Luck", 2.0), ("Will", 2.0)]),
            age_days: Some(age),
        });
    }
    let trained = train(&rows, &item_slots(), TrainOptions { min_pair_support: 30, alpha: 1.0 }).unwrap();
    let test_rolls = rolls(&[("Strength", 2.0), ("Luck", 2.0), ("Will", 2.0)]);
    let fresh = trained.predict_value("Helm_5001", &test_rolls, None, None);
    let old = trained.predict_value("Helm_5001", &test_rolls, None, Some(6.0));
    assert!((fresh - 600.0).abs() / 600.0 < 0.06);
    assert!((old / fresh - 1.2).abs() < 0.06);
}

#[test]
fn items_from_a_group_with_no_market_data_get_no_value() {
    let model = train_model(&market(1, 320, 1.0, 0));
    let guess = model.predict("Bandage_2001", &[], 1, Some("utility"), None);
    assert_eq!(guess.confidence, market::Confidence::Unknown);
}

#[test]
fn the_lowest_reasonable_price_is_the_low_end_of_asks_for_the_exact_rolls() {
    let rows = market(4, 900, 1.0, 0);
    let model = train_model(&rows);
    let est = model.predict("Helm_5001", &rolls(&[("Strength", 3.0), ("Luck", 2.0), ("Will", 2.0)]), 1, None, None);
    assert!(est.floor < est.value);
    assert!(0.8 < est.floor / est.value && est.floor / est.value < 0.97); // 8% noise: the 20th percentile sits a little under the norm
    let same: Vec<i64> = rows
        .iter()
        .filter(|r| {
            r.item_id == "Helm_5001"
                && r.rolls.iter().any(|(s, v)| s == "Strength" && *v == 3.0)
                && !r.rolls.iter().any(|(s, _)| s == "UndeadDamageMod")
                && r.rolls.len() == 3
        })
        .map(|r| r.price)
        .collect();
    let under_floor = same.iter().filter(|&&p| (p as f64) < est.floor).count();
    let below = under_floor as f64 / same.len() as f64;
    // Roughly a fifth of real asks should sit under the floor.
    assert!(0.08 < below && below < 0.35, "{under_floor} of {} same-roll listings sit under the floor ({below})", same.len());
}
