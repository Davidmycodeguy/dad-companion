//! Port of DnDTools' `tests/test_roll_pricing.py`.

use std::collections::{BTreeSet, HashMap};

use market::{price_from_market, ListerRules, MarketRow, RollPrice, EXTRA_ROLL_SHARE};

const OURS_ITEM: &str = "HeaterShield_5001";

fn rules() -> ListerRules {
    ListerRules { min_price: 50, min_listings: 1, ..ListerRules::default() }
}

fn stats(pairs: &[(&str, i64)]) -> Vec<(String, i64)> {
    pairs.iter().map(|(s, v)| (s.to_string(), *v)).collect()
}

fn ours_base() -> Vec<(String, i64)> {
    stats(&[("ArmorRating", 30)])
}

fn ours_rolls() -> Vec<(String, i64)> {
    stats(&[("Luck", 17), ("MagicalPower", 2)])
}

/// Port of the Python tests' `_row` helper (defaults: our item, our base, our rolls, no id).
struct Row {
    item: String,
    price: i64,
    base: Vec<(String, i64)>,
    rolls: Vec<(String, i64)>,
    listing_id: String,
}

impl Row {
    fn new(price: i64) -> Self {
        Row { item: OURS_ITEM.to_string(), price, base: ours_base(), rolls: ours_rolls(), listing_id: String::new() }
    }
    fn rolls(mut self, rolls: &[(&str, i64)]) -> Self {
        self.rolls = stats(rolls);
        self
    }
    fn base(mut self, base: &[(&str, i64)]) -> Self {
        self.base = stats(base);
        self
    }
    fn item(mut self, item: &str) -> Self {
        self.item = item.to_string();
        self
    }
    fn listing_id(mut self, id: &str) -> Self {
        self.listing_id = id.to_string();
        self
    }
    fn build(self) -> MarketRow {
        MarketRow { item_id: self.item, price: self.price, base: self.base, rolls: self.rolls, listing_id: self.listing_id, count: 1 }
    }
}

fn r(price: i64) -> Row {
    Row::new(price)
}

/// A `MarketRow` for an item with no base/rolls (e.g. jewellery, consumables).
fn plain_row(item: &str, price: i64) -> MarketRow {
    MarketRow { item_id: item.to_string(), price, base: vec![], rolls: vec![], listing_id: String::new(), count: 1 }
}

/// A `MarketRow` for a stack (no base/rolls), with an explicit listing id and stack size.
fn stack_row(item: &str, price: i64, listing_id: &str, count: i64) -> MarketRow {
    MarketRow { item_id: item.to_string(), price, base: vec![], rolls: vec![], listing_id: listing_id.to_string(), count }
}

/// Port of the Python tests' `_price` helper (defaults: our item/base/rolls, vendor 10, no
/// `same_rows`, a 0.5 extra-roll share).
struct Price {
    rolls: Vec<(String, i64)>,
    vendor: i64,
    same: Vec<MarketRow>,
    extra_share: f64,
}

impl Price {
    fn new() -> Self {
        Price { rolls: ours_rolls(), vendor: 10, same: vec![], extra_share: 0.5 }
    }
    fn rolls(mut self, rolls: &[(&str, i64)]) -> Self {
        self.rolls = stats(rolls);
        self
    }
    fn vendor(mut self, vendor: i64) -> Self {
        self.vendor = vendor;
        self
    }
    fn same(mut self, same: Vec<MarketRow>) -> Self {
        self.same = same;
        self
    }
    fn extra_share(mut self, share: f64) -> Self {
        self.extra_share = share;
        self
    }
    fn price(self, rows: Vec<MarketRow>) -> RollPrice {
        price_from_market(
            OURS_ITEM,
            &ours_base(),
            &self.rolls,
            self.vendor,
            &self.same,
            &rows,
            &rules(),
            self.extra_share,
            1,
            None,
            None,
            None,
            None,
        )
    }
}

fn p() -> Price {
    Price::new()
}

#[test]
fn capped_by_a_copy_that_is_at_least_as_good_on_every_stat() {
    let rows = vec![
        r(200).rolls(&[("Luck", 10), ("MagicalPower", 2)]).build(),
        r(400).rolls(&[("Luck", 17), ("MagicalPower", 3)]).build(),
        r(500).rolls(&[("Luck", 20), ("MagicalPower", 3)]).build(),
    ];
    let result = p().price(rows);
    assert_eq!((result.ok, result.price, result.flag.as_str(), result.confidence.as_str()), (true, Some(360), "", "high"));
}

#[test]
fn roll_between_two_rungs_is_priced_at_the_low_rung_not_inside_the_range() {
    let rows = vec![
        r(450).rolls(&[("Luck", 18), ("Strength", 1)]).build(),
        r(300).rolls(&[("Luck", 12), ("Agility", 2)]).build(),
        r(180).rolls(&[("MagicalPower", 2), ("Vigor", 1)]).build(),
        r(120).rolls(&[("Strength", 3)]).build(),
    ];
    let result = p().extra_share(0.0).price(rows);
    // The cheapest real listing at Luck 12 or better (300g), -10%.
    assert_eq!(result.price, Some(270));
    assert!(result.compared.contains("cheapest listing with Luck \u{2265} 12 asks 300g"));
    assert_eq!(result.confidence, "high");
}

#[test]
fn extra_good_rolls_add_a_share_of_their_premium() {
    let rows = vec![
        r(400).rolls(&[("Luck", 17), ("Strength", 1)]).build(),
        r(300).rolls(&[("MagicalPower", 2), ("Strength", 1)]).build(),
        r(700).rolls(&[("Luck", 25), ("Strength", 1)]).build(),
        r(100).rolls(&[("Strength", 1)]).build(),
        r(110).rolls(&[("Strength", 2)]).build(),
        r(120).rolls(&[("Strength", 3)]).build(),
    ];
    let with_bonus = p().extra_share(0.5).price(rows.clone());
    // Luck 400g + half of Magical Power's 200g premium over the cheapest plain copy (100g) -> 500 -> 450.
    assert_eq!(with_bonus.price, Some(450));
    assert!(with_bonus.compared.contains("extra rolls (MagicalPower 2)"));
    assert_eq!(p().extra_share(0.0).price(rows).price, Some(360));
}

#[test]
fn flags_when_our_best_roll_beats_everything_listed() {
    let rows = vec![
        r(200).rolls(&[("Luck", 10), ("MagicalPower", 1)]).build(),
        r(300).rolls(&[("Luck", 12), ("MagicalPower", 2)]).build(),
    ];
    let result = p().price(rows);
    assert!(result.ok && result.flag.contains("beat everything"));
}

#[test]
fn no_listing_with_any_of_our_rolls_falls_back_to_all_rolls() {
    let rows = vec![r(300).rolls(&[("Strength", 2)]).build(), r(350).rolls(&[("Agility", 1)]).build()];
    let result = p().price(rows);
    assert_eq!((result.ok, result.price, result.confidence.as_str()), (true, Some(270), "low"));
    assert!(result.flag.contains("no listings with rolls like yours"));
}

#[test]
fn items_without_rolls_use_plain_cheapest_price() {
    let rows = vec![plain_row("GoldBand_3001", 75), plain_row("GoldBand_3001", 125)];
    let result =
        price_from_market("GoldBand_3001", &[], &[], 10, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, None);
    assert_eq!((result.ok, result.price, result.flag.as_str()), (true, Some(67), ""));
}

#[test]
fn base_stats_within_tolerance_count_as_equal() {
    // 29 vs our 30 is close enough.
    let rows1 = vec![r(300).base(&[("ArmorRating", 29)]).build()];
    assert_eq!(p().price(rows1).price, Some(270));
    // 25 armour is clearly worse, so the 420g copy sets the price.
    let rows2 = vec![r(250).base(&[("ArmorRating", 25)]).build(), r(420).base(&[("ArmorRating", 31)]).build()];
    assert_eq!(p().price(rows2).price, Some(378));
}

#[test]
fn rows_for_other_items_are_ignored_and_no_rows_skips() {
    let rows = vec![r(100).item("Other_1").build()];
    let result = p().price(rows);
    assert_eq!((result.ok, result.reason.as_str()), (false, "nobody is selling this right now"));
}

#[test]
fn vendor_floor_still_applies() {
    let rows = vec![r(60).build()];
    assert_eq!(p().vendor(100).price(rows).reason, "vendor pays more");
}

#[test]
fn same_attribute_search_results_are_used_too() {
    let same = vec![r(900).rolls(&[("Luck", 18), ("MagicalPower", 2)]).build()];
    let rows = vec![r(300).rolls(&[("Strength", 2)]).build()];
    let result = p().same(same).price(rows);
    assert_eq!((result.price, result.flag.as_str()), (Some(810), ""));
}

#[test]
fn a_lone_lowball_listing_does_not_set_the_price() {
    let rows: Vec<MarketRow> =
        [100, 199, 222, 290, 300, 300, 311, 311, 350, 350].iter().map(|&price| r(price).rolls(&[("Strength", 1)]).build()).collect();
    let result = p().price(rows);
    assert_eq!(result.price, Some(179));
    assert!(result.compared.contains("cheapest 199g"));
}

#[test]
fn duplicate_rows_from_both_searches_count_once() {
    let row = r(400).rolls(&[("Luck", 17), ("MagicalPower", 3)]).listing_id("42").build();
    let with_same = p().same(vec![row.clone()]).price(vec![row.clone()]);
    let without_same = p().price(vec![row]);
    assert_eq!(with_same.price, without_same.price);
}

#[test]
fn closest_stronger_roll_is_preferred_over_a_god_roll() {
    let rows = vec![r(450).rolls(&[("Luck", 30)]).build(), r(300).rolls(&[("Luck", 19)]).build()];
    assert_eq!(p().price(rows).price, Some(270));
}

#[test]
fn only_much_stronger_rolls_do_not_set_the_price() {
    let rows = vec![r(450).rolls(&[("Luck", 30)]).build(), r(200).rolls(&[("Strength", 2)]).build()];
    // Luck 30 is no comparison for our Luck 17: priced as a copy of any roll.
    let result = p().price(rows);
    assert_eq!((result.price, result.confidence.as_str()), (Some(180), "low"));
    assert!(result.flag.contains("rolls like yours"));
}

#[test]
fn a_close_weaker_roll_anchors_the_price() {
    // Luck 12 is within 1.5x of our 17: its 300g sets the price.
    let rows = vec![r(450).rolls(&[("Luck", 30)]).build(), r(300).rolls(&[("Luck", 12)]).build()];
    assert_eq!(p().price(rows).price, Some(270));
}

#[test]
fn a_much_weaker_roll_does_not_drag_the_price_down() {
    // Luck 5 is no comparison for Luck 17; Luck 18 at 500g is.
    let rows = vec![r(100).rolls(&[("Luck", 5)]).build(), r(500).rolls(&[("Luck", 18)]).build()];
    assert_eq!(p().price(rows).price, Some(450));
}

#[test]
fn a_cheaper_stronger_copy_undercuts_similar_ones() {
    // Nobody pays 500g for Luck 17 while Luck 30 costs 350g.
    let rows = vec![r(500).rolls(&[("Luck", 18)]).build(), r(350).rolls(&[("Luck", 30)]).build()];
    assert_eq!(p().price(rows).price, Some(315));
}

#[test]
fn cheap_weak_rolls_are_a_real_price_level_not_lowballs() {
    let rows = vec![
        r(800).rolls(&[("Luck", 12)]).build(),
        r(900).rolls(&[("Luck", 13)]).build(),
        r(2000).rolls(&[("Luck", 17)]).build(),
        r(3000).rolls(&[("Luck", 18)]).build(),
        r(3200).rolls(&[("Luck", 18)]).build(),
    ];
    // Not 1800: expensive high rolls don't make 800g a lowball.
    assert_eq!(p().rolls(&[("Luck", 12)]).price(rows).price, Some(720));
}

#[test]
fn a_strong_roll_dumped_far_below_weaker_copies_is_a_lowball() {
    let rows = vec![
        r(800).rolls(&[("Luck", 17)]).build(),
        r(850).rolls(&[("Luck", 17)]).build(),
        r(900).rolls(&[("Luck", 18)]).build(),
        r(200).rolls(&[("Luck", 24)]).build(),
    ];
    // The 200g Luck 24 is a lowball: ignored, not undercut.
    assert_eq!(p().price(rows).price, Some(720));
}

#[test]
fn worse_base_stats_do_not_count_on_the_ladder() {
    let rows = vec![
        r(450).rolls(&[("Luck", 18)]).base(&[("ArmorRating", 20)]).build(),
        r(200).rolls(&[("Luck", 17)]).base(&[("ArmorRating", 30)]).build(),
    ];
    assert_eq!(p().price(rows).price, Some(180));
}

#[test]
fn stacks_are_priced_per_unit_times_our_quantity() {
    let rows = vec![
        stack_row("Bandage_2001", 300, "1", 3), // 100 each
        stack_row("Bandage_2001", 110, "2", 1), // 110 each
        stack_row("Bandage_2001", 500, "3", 4), // 125 each
    ];
    let result =
        price_from_market("Bandage_2001", &[], &[], 5, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 3, None, None, None, None);
    assert_eq!((result.ok, result.price), (true, Some(270))); // 100/unit, -10%, x3
    assert!(result.compared.contains("per unit"));
}

#[test]
fn one_expensive_listing_does_not_inflate_the_price() {
    let rows: Vec<MarketRow> = [1000, 1050, 1100, 1200, 15000].iter().map(|&price| plain_row("GoldBand_3001", price)).collect();
    let result =
        price_from_market("GoldBand_3001", &[], &[], 10, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, None);
    assert_eq!(result.price, Some(900));
}

#[test]
fn two_listings_far_apart_use_the_cheaper_one() {
    let rows = vec![plain_row("GoldBand_3001", 1000), plain_row("GoldBand_3001", 50000)];
    let result =
        price_from_market("GoldBand_3001", &[], &[], 10, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, None);
    assert_eq!(result.price, Some(900));
}

#[test]
fn rolled_item_ignores_a_wild_high_ask() {
    let rows = vec![r(900).build(), r(950).build(), r(99999).build()];
    assert_eq!(p().price(rows).price, Some(810));
}

#[test]
fn equal_roll_values_break_ties_on_the_cheaper_listing() {
    // We beat both listings on Luck; they have the same value, so the cheaper ask is the reference.
    let rows = vec![r(500).rolls(&[("Luck", 12)]).build(), r(50000).rolls(&[("Luck", 12)]).build()];
    assert!(p().price(rows).price.unwrap() < 1000);
}

#[test]
fn extra_roll_bonus_never_exceeds_the_most_expensive_comparable_listing() {
    let rows = vec![
        r(1000).rolls(&[("Luck", 17), ("Strength", 1)]).build(),
        r(900).rolls(&[("MagicalPower", 2), ("Strength", 1)]).build(),
        r(100).rolls(&[("Strength", 1)]).build(),
        r(110).rolls(&[("Strength", 2)]).build(),
        r(120).rolls(&[("Strength", 3)]).build(),
    ];
    let result = p().extra_share(1.0).price(rows);
    // Capped at the dearest listing (1000g) before the 10% undercut.
    assert!(result.price.unwrap() <= 900);
}

#[test]
fn learned_stat_pair_synergy_sets_the_extra_roll_bonus() {
    let rows = vec![
        r(400).rolls(&[("Luck", 17), ("Strength", 1)]).build(),
        r(300).rolls(&[("MagicalPower", 2), ("Strength", 1)]).build(),
        r(900).rolls(&[("Luck", 25), ("Strength", 1)]).build(),
        r(100).rolls(&[("Strength", 1)]).build(),
        r(110).rolls(&[("Strength", 2)]).build(),
        r(120).rolls(&[("Strength", 3)]).build(),
    ];
    let synergies: HashMap<BTreeSet<String>, f64> =
        HashMap::from([(BTreeSet::from(["Luck".to_string(), "MagicalPower".to_string()]), 20.0)]);
    let result = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &ours_rolls(),
        10,
        &[],
        &rows,
        &rules(),
        0.0,
        1,
        Some(&synergies),
        None,
        None,
        None,
    );
    // Luck 400g +20% for the Luck + MagicalPower pair -> 480 -> 432.
    assert_eq!(result.price, Some(432));
    assert!(result.compared.contains("pair"));
}

#[test]
fn a_lowball_better_copy_does_not_cap_the_price() {
    let rows = vec![
        r(700).rolls(&[("Luck", 15), ("MagicalPower", 2)]).build(),
        r(750).rolls(&[("Luck", 16), ("MagicalPower", 1)]).build(),
        r(800).rolls(&[("Luck", 17), ("MagicalPower", 2)]).build(),
        r(60).rolls(&[("Luck", 19), ("MagicalPower", 3)]).build(),
    ];
    // Not 54: the 60g god roll is a lowball, not a ceiling.
    assert_eq!(p().price(rows).price, Some(675));
}

#[test]
fn a_lowball_one_step_below_our_roll_is_ignored() {
    let rows =
        vec![r(200).rolls(&[("Luck", 16)]).build(), r(800).rolls(&[("Luck", 17)]).build(), r(900).rolls(&[("Luck", 18)]).build()];
    // 200g for Luck 16 when Luck 17 asks 800g is a dump.
    assert_eq!(p().price(rows).price, Some(720));
}

#[test]
fn two_listings_far_apart_are_flagged_for_review() {
    let rows = vec![plain_row("GoldBand_3001", 100), plain_row("GoldBand_3001", 1600)];
    let result =
        price_from_market("GoldBand_3001", &[], &[], 10, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, None);
    assert_eq!((result.price, result.confidence.as_str()), (Some(90), "low"));
    assert!(result.flag.contains("far apart"));
}

#[test]
fn tied_rolls_give_the_same_result_in_any_order() {
    let rows = || {
        vec![
            r(200).rolls(&[("Luck", 10), ("MagicalPower", 1)]).build(),
            r(300).rolls(&[("Luck", 12), ("MagicalPower", 2)]).build(),
        ]
    };
    let forward = p().rolls(&[("Luck", 17), ("MagicalPower", 2)]).price(rows());
    let backward = p().rolls(&[("MagicalPower", 2), ("Luck", 17)]).price(rows());
    assert_eq!(forward.price, backward.price);
    assert_eq!(forward.confidence, backward.confidence);
    assert_eq!(forward.flag, backward.flag);
    // Still told that one roll beats every listing.
    assert!(forward.flag.contains("beat everything"));
}

#[test]
fn the_value_model_caps_a_price_inherited_from_unrelated_stats() {
    let rows = || {
        vec![
            r(700).rolls(&[("Luck", 17), ("Vigor", 3)]).build(),
            r(300).rolls(&[("Strength", 1)]).build(),
            r(320).rolls(&[("Will", 1)]).build(),
        ]
    };
    let luck_only = stats(&[("Luck", 17)]);
    let plain =
        price_from_market(OURS_ITEM, &ours_base(), &luck_only, 10, &[], &rows(), &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, None);
    let capped = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &luck_only,
        10,
        &[],
        &rows(),
        &rules(),
        EXTRA_ROLL_SHARE,
        1,
        None,
        Some(400.0),
        None,
        None,
    );
    assert_eq!(plain.price, Some(630));
    assert_eq!(capped.price, Some(360));
    assert!(capped.compared.contains("value model"));
    assert_eq!(capped.flag, "");
}

#[test]
fn the_value_model_never_prices_below_the_cheapest_real_listing() {
    let rows = vec![r(700).rolls(&[("Luck", 17)]).build(), r(300).rolls(&[("Strength", 1)]).build(), r(320).rolls(&[("Will", 1)]).build()];
    let luck_only = stats(&[("Luck", 17)]);
    let result = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &luck_only,
        10,
        &[],
        &rows,
        &rules(),
        EXTRA_ROLL_SHARE,
        1,
        None,
        Some(100.0),
        None,
        None,
    );
    // Floored at the cheapest listing of any roll (300g), then undercut.
    assert_eq!(result.price, Some(270));
}

#[test]
fn a_higher_model_value_never_raises_the_market_price() {
    let rows = vec![r(700).rolls(&[("Luck", 17)]).build(), r(300).rolls(&[("Strength", 1)]).build()];
    let luck_only = stats(&[("Luck", 17)]);
    let result = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &luck_only,
        10,
        &[],
        &rows,
        &rules(),
        EXTRA_ROLL_SHARE,
        1,
        None,
        Some(5000.0),
        None,
        None,
    );
    assert_eq!(result.price, Some(630));
}

#[test]
fn fast_sale_lists_at_the_lowest_reasonable_price() {
    let rows = vec![r(700).rolls(&[("Luck", 17)]).build(), r(300).rolls(&[("Strength", 1)]).build()];
    let luck_only = stats(&[("Luck", 17)]);
    let result = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &luck_only,
        10,
        &[],
        &rows,
        &rules(),
        EXTRA_ROLL_SHARE,
        1,
        None,
        Some(800.0),
        Some(560.0),
        None,
    );
    // Under the 630 undercut.
    assert_eq!(result.price, Some(560));
    assert!(result.compared.contains("lowest reasonable"));
}

#[test]
fn a_cheaper_comparable_listing_is_still_undercut() {
    let rows = vec![r(400).rolls(&[("Luck", 17)]).build(), r(300).rolls(&[("Strength", 1)]).build()];
    let luck_only = stats(&[("Luck", 17)]);
    let result = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &luck_only,
        10,
        &[],
        &rows,
        &rules(),
        EXTRA_ROLL_SHARE,
        1,
        None,
        Some(600.0),
        Some(480.0),
        None,
    );
    // 10% under the 400g copy beats waiting at the floor.
    assert_eq!(result.price, Some(360));
}

#[test]
fn fast_sale_never_goes_below_half_the_floor() {
    let rows =
        vec![r(120).rolls(&[("Luck", 17)]).build(), r(110).rolls(&[("Strength", 1)]).build(), r(130).rolls(&[("Will", 1)]).build()];
    let luck_only = stats(&[("Luck", 17)]);
    let result = price_from_market(
        OURS_ITEM,
        &ours_base(),
        &luck_only,
        10,
        &[],
        &rows,
        &rules(),
        EXTRA_ROLL_SHARE,
        1,
        None,
        Some(500.0),
        Some(400.0),
        None,
    );
    // The market's 120g copies are dumps next to a 400g floor.
    assert_eq!(result.price, Some(200));
}

#[test]
fn stacks_are_compared_with_stacks_and_singles_with_singles() {
    let rows = vec![stack_row("Bolt_2001", 753, "1", 20), stack_row("Bolt_2001", 89, "2", 1), stack_row("Bolt_2001", 52499, "3", 1)];
    let stack = price_from_market("Bolt_2001", &[], &[], 1, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 20, None, None, None, None);
    let single = price_from_market("Bolt_2001", &[], &[], 1, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, None);
    assert_eq!(stack.price, Some(677)); // 753g for 20 is the bulk price: 37.65g each, -10%.
    assert_eq!(single.price, Some(80)); // A single bolt competes with single bolts (89g).
}

#[test]
fn nobody_pays_more_than_the_merchant_shop_price() {
    let rows = vec![stack_row("Bolt_2001", 753, "1", 20)];
    let result =
        price_from_market("Bolt_2001", &[], &[], 0, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 20, None, None, None, Some(2.0));
    assert!(!result.ok);
    assert_eq!(result.reason, "a merchant sells it for 40g");
}

#[test]
fn a_merchant_price_above_the_market_changes_nothing() {
    let rows = vec![stack_row("Potion_2001", 100, "1", 1), stack_row("Potion_2001", 110, "2", 1)];
    let result =
        price_from_market("Potion_2001", &[], &[], 0, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, Some(150.0));
    assert_eq!(result.price, Some(90));
}

#[test]
fn a_merchant_price_below_the_market_caps_the_listing() {
    let rows = vec![stack_row("Potion_2001", 200, "1", 1), stack_row("Potion_2001", 210, "2", 1)];
    let result =
        price_from_market("Potion_2001", &[], &[], 0, &[], &rows, &rules(), EXTRA_ROLL_SHARE, 1, None, None, None, Some(120.0));
    assert_eq!(result.price, Some(108));
    assert!(result.compared.contains("merchant"));
}
