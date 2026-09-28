//! Recording what the game shows into the market history, ported with the Python app's tests
//! (tests/test_market_history.py).

use market::history::{item_id_from_design, stat_from_property, MerchantOffer, MyListing, PageListing};
use market::{MarketHistory, Stat, DAY_S};

const DAY_MS: i64 = 86_400_000;
const T0: f64 = 1_000_000.0;

fn history() -> (tempfile::TempDir, MarketHistory) {
    let dir = tempfile::tempdir().unwrap();
    let history = MarketHistory::open(&dir.path().join("history.sqlite")).unwrap();
    (dir, history)
}

/// (listing id, item, price, remaining ms, luck roll)
fn page(listings: &[(u64, &str, i64, i64, i64)]) -> Vec<PageListing> {
    listings
        .iter()
        .map(|&(id, item, price, remain_ms, luck)| PageListing {
            listing_id: id.to_string(),
            item_id: item.to_owned(),
            price,
            count: 1,
            base: vec![],
            rolls: vec![Stat { id: "Luck".into(), value: luck }],
            seller: "seller1".into(),
            remain_ms,
        })
        .collect()
}

fn ids(listings: &[market::Listing]) -> Vec<String> {
    listings.iter().map(|l| l.listing_id.clone()).collect()
}

#[test]
fn game_ids_become_the_catalogs() {
    assert_eq!(item_id_from_design("DesignDataItem:Id_Item_HeaterShield_5001"), "HeaterShield_5001");
    assert_eq!(item_id_from_design("HeaterShield_5001"), "HeaterShield_5001");
    assert_eq!(stat_from_property("DesignDataItemPropertyType:Id_ItemPropertyType_Effect_Luck"), "Luck");
}

#[test]
fn recorded_listings_are_open_until_they_expire() {
    let (_dir, history) = history();
    history
        .record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17), (2, "HeaterShield_5001", 350, 1000, 12)]), T0)
        .unwrap(); // #2 expires in 1 s
    let open = history.open_listings("HeaterShield_5001", T0 + 60.0, 3600.0).unwrap();
    assert_eq!(ids(&open), ["1"]);
    assert_eq!(open[0].rolls, [Stat { id: "Luck".into(), value: 17 }]);
}

#[test]
fn a_scan_marks_listings_that_vanished_before_expiry() {
    let (_dir, history) = history();
    history
        .record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17), (2, "HeaterShield_5001", 900, 5 * DAY_MS, 20)]), T0)
        .unwrap();
    let started = T0 + 3600.0;
    history.record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17)]), started).unwrap(); // #2 is gone
    // 900g is beyond what this scan covered, so it can't tell.
    assert_eq!(history.note_scan("HeaterShield_5001", started, 500, false, started).unwrap(), 0);
    assert_eq!(history.note_scan("HeaterShield_5001", started, 500, true, started).unwrap(), 1);
    assert_eq!(ids(&history.probable_sales("HeaterShield_5001", T0).unwrap()), ["2"]);
    assert_eq!(history.summary().unwrap().vanished, 1);
}

#[test]
fn an_incomplete_scan_does_not_vanish_listings_at_its_price_boundary() {
    // An incomplete scan that stopped at 500g may have cut off other 500g listings mid-page.
    let (_dir, history) = history();
    history
        .record_listings(
            &page(&[
                (1, "HeaterShield_5001", 300, 5 * DAY_MS, 17),
                (2, "HeaterShield_5001", 500, 5 * DAY_MS, 20),
                (3, "HeaterShield_5001", 400, 5 * DAY_MS, 12),
            ]),
            T0,
        )
        .unwrap();
    let started = T0 + 3600.0;
    history.record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17)]), started).unwrap();
    assert_eq!(history.note_scan("HeaterShield_5001", started, 500, false, started).unwrap(), 1); // only the 400g
    assert_eq!(ids(&history.probable_sales("HeaterShield_5001", T0).unwrap()), ["3"]);
}

#[test]
fn a_second_connection_can_read_while_one_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite");
    let (writer, reader) = (MarketHistory::open(&path).unwrap(), MarketHistory::open(&path).unwrap());
    writer.record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17)]), T0).unwrap();
    assert_eq!(reader.summary().unwrap().listings, 1);
}

#[test]
fn a_listing_seen_again_is_not_vanished_and_keeps_its_new_price() {
    let (_dir, history) = history();
    history.record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17)]), T0).unwrap();
    let started = T0 + 60.0;
    history.record_listings(&page(&[(1, "HeaterShield_5001", 290, 5 * DAY_MS, 17)]), started).unwrap(); // price cut
    assert_eq!(history.note_scan("HeaterShield_5001", started, 1000, true, started).unwrap(), 0);
    assert_eq!(history.open_listings("HeaterShield_5001", started, 3600.0).unwrap()[0].price, 290);
}

#[test]
fn my_listings_record_when_ours_sold() {
    let (_dir, history) = history();
    let mine = |state| vec![MyListing { listing_id: "41875349".into(), item_id: "GreatHelm_3001".into(), price: 200, state }];
    history.record_my_listings(&mine(1), T0).unwrap();
    history.record_my_listings(&mine(3), T0 + 50.0).unwrap();
    let summary = history.summary().unwrap();
    assert_eq!(summary.my_sold, 1);
    assert_eq!(history.my_sold_at("41875349").unwrap(), Some(T0 + 50.0));
}

#[test]
fn a_full_crawl_pass_marks_listings_that_were_not_seen_again() {
    let (_dir, history) = history();
    history
        .record_listings(
            &page(&[
                (1, "HeaterShield_5001", 300, 5 * DAY_MS, 17),
                (2, "GreatHelm_5001", 900, 5 * DAY_MS, 20),
                (3, "GemRing_4001", 100, 5 * DAY_MS, 1),     // another rarity
                (4, "HeaterShield_5001", 350, 60_000, 12), // expires in a minute
            ]),
            T0,
        )
        .unwrap();
    let started = T0 + 3600.0;
    history.record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17)]), started).unwrap(); // the pass sees only #1
    assert_eq!(history.note_crawl_pass(5, started, started + 60.0).unwrap(), 1); // #2 gone; #3 Rare; #4 expired
    assert_eq!(ids(&history.probable_sales("GreatHelm_5001", T0).unwrap()), ["2"]);
    assert!(history.probable_sales("HeaterShield_5001", T0).unwrap().is_empty());
}

#[test]
fn my_listing_ids_are_every_listing_of_ours_still_up() {
    let (_dir, history) = history();
    let rows: Vec<MyListing> = [(501, 1), (502, 3), (503, 1)]
        .iter()
        .map(|&(id, state)| MyListing { listing_id: id.to_string(), item_id: "GemRing_5001".into(), price: 100, state })
        .collect();
    history.record_my_listings(&rows, T0).unwrap();
    let mut ids: Vec<String> = history.my_listing_ids().unwrap().into_iter().collect();
    ids.sort();
    assert_eq!(ids, ["501", "503"]); // 502 has sold
}

#[test]
fn merchant_prices_are_per_unit_keeping_the_cheapest() {
    let (_dir, history) = history();
    let offer = |item: &str, count, price| MerchantOffer { item_id: item.into(), count, final_price: price };
    history.record_merchant_stock(&[offer("Bolt_2001", 20, 40), offer("Bandage_2001", 1, 15)], T0).unwrap();
    history.record_merchant_stock(&[offer("Bandage_2001", 1, 12)], T0 + 1.0).unwrap(); // sold cheaper elsewhere
    assert_eq!(history.merchant_price("Bolt_2001").unwrap(), Some(2.0));
    assert_eq!(history.merchant_price("Bandage_2001").unwrap(), Some(12.0));
    assert_eq!(history.merchant_price("Ale_2001").unwrap(), None);
    assert_eq!(history.merchant_prices().unwrap().len(), 2);
}

#[test]
fn sales_count_listings_that_vanished_recently() {
    let (_dir, history) = history();
    history
        .record_listings(&page(&[(1, "HeaterShield_5001", 300, 5 * DAY_MS, 17), (2, "HeaterShield_5001", 320, 5 * DAY_MS, 12)]), T0)
        .unwrap();
    let started = T0 + 3600.0;
    history.record_listings(&page(&[(2, "HeaterShield_5001", 320, 5 * DAY_MS, 12)]), started).unwrap();
    history.note_scan("HeaterShield_5001", started, 500, true, started).unwrap(); // #1 sold now
    assert_eq!(history.probable_sales("HeaterShield_5001", started - 60.0).unwrap().len(), 1);
    assert_eq!(history.probable_sales("HeaterShield_5001", started + 60.0).unwrap().len(), 0);
    assert!(history.probable_sales("GemRing_5001", 0.0).unwrap().is_empty());
    let _ = DAY_S;
}

#[test]
fn newest_seen_is_the_latest_sighting() {
    let (_dir, history) = history();
    assert_eq!(history.newest_seen().unwrap(), None);
    history.record_listings(&page(&[(1, "HeaterShield_4001", 100, 3_600_000, 1)]), T0).unwrap();
    history.record_listings(&page(&[(2, "HeaterShield_4001", 200, 3_600_000, 2)]), T0 + 60.0).unwrap();
    assert_eq!(history.newest_seen().unwrap(), Some(T0 + 60.0));
}

#[test]
fn the_starter_export_drops_sellers_and_our_own_listings() {
    let (dir, history) = history();
    history.record_listings(&page(&[(1, "HeaterShield_4001", 100, 3_600_000, 1), (2, "HeaterShield_4001", 200, 3_600_000, 2)]), T0).unwrap();
    history.record_my_listings(&[MyListing { listing_id: "2".into(), item_id: "HeaterShield_4001".into(), price: 200, state: 1 }], T0).unwrap();
    history.record_merchant_stock(&[MerchantOffer { item_id: "HeaterShield_4001".into(), count: 1, final_price: 90 }], T0).unwrap();

    let dest = dir.path().join("starter.sqlite");
    let summary = history.export_starter(&dest, T0 - 1.0).unwrap();
    assert_eq!(summary.listings, 1);

    let starter = MarketHistory::open(&dest).unwrap();
    assert_eq!(ids(&starter.open_listings("HeaterShield_4001", T0, 3600.0).unwrap()), ["1"]);
    assert!(starter.my_listing_ids().unwrap().is_empty());
    assert_eq!(starter.merchant_price("HeaterShield_4001").unwrap(), Some(90.0));
    let sellers: i64 = rusqlite::Connection::open(&dest)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM listings WHERE seller != ''", [], |row| row.get(0))
        .unwrap();
    assert_eq!(sellers, 0);
}

#[test]
fn the_starter_export_keeps_only_recent_listings() {
    let (dir, history) = history();
    history.record_listings(&page(&[(1, "HeaterShield_4001", 100, 3_600_000, 1)]), T0 - 10_000.0).unwrap();
    history.record_listings(&page(&[(2, "HeaterShield_4001", 200, 3_600_000, 2)]), T0).unwrap();
    let summary = history.export_starter(&dir.path().join("starter.sqlite"), T0 - 1.0).unwrap();
    assert_eq!(summary.listings, 1);
}
