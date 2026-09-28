//! The hover card's numbers, ported with the Python app's tests (tests/test_hover_card.py).

use market::card::{build_card, market_pace, round_gold, ItemFacts, MarketFacts, TipFacts, Verdict};
use market::{listing_fee, Confidence, Estimate, PairWorth, RollWorth};

fn tip() -> TipFacts {
    TipFacts {
        title: "Dark Cuirass".into(),
        rarity: "Epic".into(),
        rolls: vec![("Strength".into(), 3), ("ArmorPenetration".into(), 42)],
        unread: 0,
    }
}

fn shop() -> ItemFacts {
    ItemFacts { kind: "Chest · Plate".into(), icon: None, merchant: 60, slots: 6 }
}

fn estimate(value: f64, floor: f64, pairs: Vec<PairWorth>) -> Estimate {
    Estimate {
        value,
        floor,
        low: 290.1,
        high: 455.0,
        confidence: Confidence::High,
        typical: 350.0,
        rolls: vec![
            RollWorth { stat: "Strength".into(), value: 3.0, quality: 0.98, effect_pct: 18.0 },
            RollWorth { stat: "ArmorPenetration".into(), value: 42.0, quality: 0.5, effect_pct: -9.5 },
        ],
        pairs,
        listings: 812,
    }
}

fn standard() -> Estimate {
    estimate(364.4, 310.0, vec![])
}

#[test]
fn list_it_when_a_fast_sale_nets_more_than_a_merchant_pays() {
    let card = build_card(&tip(), Some(&standard()), &MarketFacts::default(), &shop());
    assert_eq!((card.value, card.low, card.high, card.confidence), (Some(364), Some(290), Some(455), Some(Confidence::High)));
    assert_eq!((card.fast_sale, card.net), (Some(310), Some(294))); // fee: 5% of 310 rounded up = 16g
    assert_eq!(card.verdict, Verdict::List);
}

#[test]
fn fast_sale_is_never_above_the_value() {
    let card = build_card(&tip(), Some(&estimate(200.0, 260.0, vec![])), &MarketFacts::default(), &shop());
    assert_eq!(card.fast_sale, Some(200));
}

#[test]
fn merchant_when_the_fee_eats_the_margin() {
    let item = ItemFacts { merchant: 50, ..ItemFacts::default() };
    let card = build_card(&tip(), Some(&estimate(60.0, 55.0, vec![])), &MarketFacts::default(), &item);
    assert_eq!((card.fast_sale, card.net), (Some(55), Some(40))); // the 15g minimum fee
    assert_eq!(card.verdict, Verdict::Merchant);
}

#[test]
fn no_estimate_means_no_data_and_the_merchant_price_carries_the_card() {
    let item = ItemFacts { merchant: 15, slots: 3, ..ItemFacts::default() };
    let card = build_card(&tip(), None, &MarketFacts::default(), &item);
    assert_eq!((card.verdict, card.value, card.fast_sale, card.net, card.quality), (Verdict::Unknown, None, None, None, None));
    assert_eq!(card.per_slot, 5);
    let rolls: Vec<_> = card.rolls.iter().map(|r| (r.quality, r.gold, r.is_max)).collect();
    assert_eq!(rolls, [(None, None, false), (None, None, false)]);
}

#[test]
fn rolls_read_like_the_game_with_quality_and_the_gold_they_add() {
    let card = build_card(&tip(), Some(&standard()), &MarketFacts::default(), &shop());
    let rolls: Vec<_> = card.rolls.iter().map(|r| (r.label.as_str(), r.value.as_str(), r.quality, r.gold, r.is_max)).collect();
    assert_eq!(
        rolls,
        [
            ("Strength", "+3", Some(0.98), Some(65), true), // 18% of a 350g typical = 63, shown as 65
            ("Armor Penetration", "+4.2%", Some(0.5), Some(-35), false),
        ]
    );
    assert_eq!(card.quality, Some(74));
}

#[test]
fn only_meaningful_synergies_are_listed() {
    let pairs = vec![
        PairWorth { stats: vec!["Strength".into(), "ArmorPenetration".into()], effect_pct: 12.0 },
        PairWorth { stats: vec!["Strength".into(), "Vigor".into()], effect_pct: 2.4 },
    ];
    let card = build_card(&tip(), Some(&estimate(364.4, 310.0, pairs)), &MarketFacts::default(), &shop());
    let pairs: Vec<_> = card.pairs.iter().map(|p| (p.label.as_str(), p.pct)).collect();
    assert_eq!(pairs, [("Strength + Armor Penetration", 12.0)]);
}

#[test]
fn market_facts_come_through_sorted_with_pace_and_trend() {
    let market = MarketFacts { asks: vec![330.4, 280.0, 512.6], sold_week: 1, trend: vec![100.0, 100.0, 110.0, 120.0], ..MarketFacts::default() };
    let card = build_card(&tip(), Some(&standard()), &market, &shop());
    assert_eq!((card.asks.as_slice(), card.sold_week, card.pace), (&[280, 330, 513][..], 1, Some("sells fast")));
    assert!((card.trend_pct.unwrap() - 15.0).abs() < 1e-9);
}

#[test]
fn a_short_price_history_has_no_trend() {
    let market = MarketFacts { trend: vec![100.0, 120.0, 140.0], ..MarketFacts::default() };
    assert_eq!(build_card(&tip(), Some(&standard()), &market, &shop()).trend_pct, None);
}

#[test]
fn value_per_inventory_slot() {
    let card = build_card(&tip(), Some(&standard()), &MarketFacts::default(), &shop());
    assert_eq!(card.per_slot, 61); // 364 over 6 slots
}

#[test]
fn market_pace_needs_a_few_listings_to_judge() {
    assert_eq!(market_pace(2, 5), None);
    assert_eq!([market_pace(4, 1), market_pace(10, 1), market_pace(10, 0)], [Some("sells fast"), Some("steady"), Some("slow market")]);
}

#[test]
fn gold_effects_are_rounded_to_what_an_estimate_can_claim() {
    assert_eq!([round_gold(7.4), round_gold(63.0), round_gold(-33.25), round_gold(187.0)], [7, 65, -35, 190]);
}

#[test]
fn older_asks_carry_how_long_ago_they_were_seen() {
    let market = MarketFacts { asks: vec![300.0], seen_ago_s: Some(7.0 * 3600.0), ..MarketFacts::default() };
    assert_eq!(build_card(&tip(), Some(&standard()), &market, &shop()).seen_ago_s, Some(7.0 * 3600.0));
}

#[test]
fn no_pace_until_sales_could_have_been_seen() {
    let asks = vec![300.0, 320.0, 340.0, 360.0];
    let untracked = MarketFacts { asks: asks.clone(), ..MarketFacts::default() };
    assert_eq!(build_card(&tip(), Some(&standard()), &untracked, &shop()).pace, None);
    let tracked = MarketFacts { asks, sales_tracked: true, ..MarketFacts::default() };
    assert_eq!(build_card(&tip(), Some(&standard()), &tracked, &shop()).pace, Some("slow market"));
}

#[test]
fn a_fast_sale_that_rounds_to_nothing_still_makes_a_card() {
    let item = ItemFacts { merchant: 5, ..ItemFacts::default() };
    let card = build_card(&tip(), Some(&estimate(0.4, 0.3, vec![])), &MarketFacts::default(), &item);
    assert_eq!((card.fast_sale, card.net, card.verdict), (Some(0), Some(-15), Verdict::Merchant));
}

#[test]
fn the_fast_sale_is_what_the_lister_would_list_at_when_known() {
    let market = MarketFacts { asks: vec![280.0, 330.0], listing_price: Some(275), ..MarketFacts::default() };
    let card = build_card(&tip(), Some(&standard()), &market, &shop());
    assert_eq!((card.fast_sale, card.net), (Some(275), Some(260)));
    let market = MarketFacts { asks: vec![280.0, 330.0], ..MarketFacts::default() };
    assert_eq!(build_card(&tip(), Some(&standard()), &market, &shop()).fast_sale, Some(310));
}

#[test]
fn the_listing_fee_is_five_percent_rounded_up_with_a_minimum() {
    assert_eq!([listing_fee(310), listing_fee(275), listing_fee(0), listing_fee(1000)], [16, 15, 15, 50]);
}
