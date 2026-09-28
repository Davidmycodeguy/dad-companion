//! Market-wide activity: which items are listed most, and which sell fastest.

use market::history::{MyListing, PageListing};
use market::MarketHistory;

const T0: f64 = 1_000_000.0;
const DAY_MS: i64 = 86_400_000;

fn history() -> (tempfile::TempDir, MarketHistory) {
    let dir = tempfile::tempdir().unwrap();
    let history = MarketHistory::open(&dir.path().join("history.sqlite")).unwrap();
    (dir, history)
}

/// (listing id, item, price, count)
fn page(listings: &[(u64, &str, i64, i64)]) -> Vec<PageListing> {
    listings
        .iter()
        .map(|&(id, item, price, count)| PageListing {
            listing_id: id.to_string(),
            item_id: item.to_owned(),
            price,
            count,
            base: vec![],
            rolls: vec![],
            seller: String::new(),
            remain_ms: 5 * DAY_MS,
        })
        .collect()
}

#[test]
fn most_listed_items_come_first_with_their_prices() {
    let (_dir, history) = history();
    history
        .record_listings(
            &page(&[(1, "Bandage_2001", 30, 3), (2, "Bandage_2001", 40, 1), (3, "Bandage_2001", 60, 1), (4, "HeaterShield_4001", 500, 1)]),
            T0,
        )
        .unwrap();
    history.record_my_listings(&[MyListing { listing_id: "3".into(), item_id: "Bandage_2001".into(), price: 60, state: 1 }], T0).unwrap();

    let top = history.most_listed(T0 + 60.0, 3600.0, 10).unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!((top[0].item_id.as_str(), top[0].count), ("Bandage_2001", 2), "our own listing is not counted");
    assert_eq!(top[0].lowest, 10.0, "unit prices: 30g for 3 is 10g each");
    assert_eq!(top[0].median, 25.0);
    assert_eq!((top[1].item_id.as_str(), top[1].count), ("HeaterShield_4001", 1));
    assert_eq!(history.most_listed(T0 + 60.0, 3600.0, 1).unwrap().len(), 1);
}

#[test]
fn listings_seen_too_long_ago_are_not_the_market() {
    let (_dir, history) = history();
    history.record_listings(&page(&[(1, "Bandage_2001", 30, 1)]), T0).unwrap();
    assert!(history.most_listed(T0 + 7200.0, 3600.0, 10).unwrap().is_empty());
}

#[test]
fn fastest_sellers_count_listings_that_vanished_before_expiry() {
    let (_dir, history) = history();
    history
        .record_listings(&page(&[(1, "Bandage_2001", 30, 1), (2, "Bandage_2001", 50, 1), (3, "HeaterShield_4001", 500, 1)]), T0)
        .unwrap();
    let started = T0 + 3600.0;
    history.record_listings(&page(&[(3, "HeaterShield_4001", 500, 1)]), started).unwrap();
    history.note_scan("Bandage_2001", started, 1000, true, started).unwrap();
    history.note_scan("HeaterShield_4001", started, 1000, true, started).unwrap();

    let sold = history.fastest_selling(T0, 10).unwrap();
    assert_eq!(sold.len(), 1);
    assert_eq!((sold[0].item_id.as_str(), sold[0].count), ("Bandage_2001", 2));
    assert_eq!((sold[0].lowest, sold[0].median), (30.0, 40.0));
    assert!(history.fastest_selling(started + 1.0, 10).unwrap().is_empty(), "sales before `since` are left out");
}

#[test]
fn pattern_listings_carry_every_saved_listing_with_its_stats() {
    let (_dir, history) = history();
    history.record_listings(&page(&[(1, "Bandage_2001", 30, 3), (2, "HeaterShield_4001", 500, 1)]), T0).unwrap();
    let listings = history.pattern_listings().unwrap();
    assert_eq!(listings.len(), 2);
    let bandages = listings.iter().find(|l| l.item_id == "Bandage_2001").unwrap();
    assert_eq!((bandages.price, bandages.item_count), (30, 3));
}

#[test]
fn listings_seen_before_a_time_are_counted() {
    let (_dir, history) = history();
    history.record_listings(&page(&[(1, "Bandage_2001", 30, 1), (2, "Bandage_2001", 40, 1)]), T0).unwrap();
    history.record_listings(&page(&[(3, "Bandage_2001", 50, 1)]), T0 + 100.0).unwrap();
    let ids: Vec<String> = ["1", "2", "3", "4"].iter().map(|s| s.to_string()).collect();
    assert_eq!(history.count_seen_before(&ids, T0 + 50.0).unwrap(), 2);
    assert_eq!(history.count_seen_before(&[], T0 + 50.0).unwrap(), 0);
}
