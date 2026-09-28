use market::{MarketHistory, Stat, DAY_S};
use rusqlite::{params, Connection};

const NOW: f64 = 1_790_000_000.0;
const HOUR: f64 = 3_600.0;

/// (listing_id, item_id, price, count, rolls json, first_seen, last_seen, expires_at, vanished_at)
type Row<'a> = (&'a str, &'a str, i64, i64, &'a str, f64, f64, f64, Option<f64>);

/// A market database with the given listings.
fn history(rows: &[Row<'_>]) -> (tempfile::TempDir, MarketHistory) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("market_history.sqlite");
    let history = MarketHistory::open(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    for (id, item, price, count, rolls, first, last, expires, vanished) in rows {
        db.execute(
            "INSERT INTO listings VALUES (?1, ?2, 5, ?3, ?4, '[[\"PhysicalWeaponDamage\", 31]]', ?5, '', ?6, ?7, ?8, ?9)",
            params![id, item, price, count, rolls, first, last, expires, vanished],
        )
        .unwrap();
    }
    (dir, history)
}

fn ids(listings: &[market::Listing]) -> Vec<&str> {
    listings.iter().map(|l| l.listing_id.as_str()).collect()
}

#[test]
fn open_listings_are_live_unexpired_not_ours_and_cheapest_first() {
    let week = 7.0 * DAY_S;
    let (dir, history) = history(&[
        ("pricey", "Sword_5001", 900, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + week, None),
        ("cheap", "Sword_5001", 300, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + week, None),
        ("stack", "Sword_5001", 1000, 4, "[]", NOW - HOUR, NOW - HOUR, NOW + week, None),
        ("sold", "Sword_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + week, Some(NOW - 60.0)),
        ("expired", "Sword_5001", 100, 1, "[]", NOW - week, NOW - week, NOW - 1.0, None),
        ("stale", "Sword_5001", 100, 1, "[]", NOW - 3.0 * DAY_S, NOW - 3.0 * DAY_S, NOW + DAY_S, None),
        ("mine", "Sword_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + week, None),
        ("other", "Axe_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + week, None),
    ]);
    Connection::open(dir.path().join("market_history.sqlite"))
        .unwrap()
        .execute("INSERT INTO my_listings VALUES ('mine', 'Sword_5001', 100, 1, ?1, ?1, NULL)", [NOW - HOUR])
        .unwrap();

    let listings = history.open_listings("Sword_5001", NOW, DAY_S).unwrap();

    assert_eq!(ids(&listings), ["stack", "cheap", "pricey"]);
    assert_eq!(listings[0].unit_price(), 250.0);
}

#[test]
fn listings_carry_their_stats() {
    let (_dir, history) = history(&[(
        "a",
        "Sword_5001",
        500,
        1,
        r#"[["ArmorPenetration", 30], ["PhysicalPower", 2]]"#,
        NOW - HOUR,
        NOW - HOUR,
        NOW + DAY_S,
        None,
    )]);

    let listing = &history.open_listings("Sword_5001", NOW, DAY_S).unwrap()[0];

    assert_eq!(listing.base, [Stat { id: "PhysicalWeaponDamage".into(), value: 31 }]);
    assert_eq!(
        listing.rolls,
        [Stat { id: "ArmorPenetration".into(), value: 30 }, Stat { id: "PhysicalPower".into(), value: 2 }]
    );
}

#[test]
fn damaged_stats_read_as_none_instead_of_failing() {
    let (_dir, history) = history(&[("a", "Sword_5001", 500, 1, "not json", NOW - HOUR, NOW - HOUR, NOW + DAY_S, None)]);

    let listing = &history.open_listings("Sword_5001", NOW, DAY_S).unwrap()[0];

    assert!(listing.rolls.is_empty());
}

#[test]
fn probable_sales_are_listings_that_vanished_since_newest_first() {
    let (_dir, history) = history(&[
        ("older_sale", "Sword_5001", 400, 1, "[]", NOW - DAY_S, NOW - DAY_S, NOW + DAY_S, Some(NOW - 10.0 * HOUR)),
        ("newer_sale", "Sword_5001", 500, 1, "[]", NOW - DAY_S, NOW - HOUR, NOW + DAY_S, Some(NOW - HOUR)),
        ("before", "Sword_5001", 600, 1, "[]", NOW - 3.0 * DAY_S, NOW - 3.0 * DAY_S, NOW, Some(NOW - 3.0 * DAY_S)),
        ("still_up", "Sword_5001", 700, 1, "[]", NOW - DAY_S, NOW - HOUR, NOW + DAY_S, None),
    ]);

    let sales = history.probable_sales("Sword_5001", NOW - 2.0 * DAY_S).unwrap();

    assert_eq!(ids(&sales), ["newer_sale", "older_sale"]);
    assert_eq!(sales[0].vanished_at, Some(NOW - HOUR));
}

#[test]
fn daily_medians_count_each_listing_on_every_day_it_was_open() {
    let today = (NOW / DAY_S).floor() * DAY_S;
    let (_dir, history) = history(&[
        // open for three days
        ("long", "Sword_5001", 300, 1, "[]", today - 2.0 * DAY_S + HOUR, today + HOUR, NOW + DAY_S, None),
        ("a", "Sword_5001", 100, 1, "[]", today - 2.0 * DAY_S + HOUR, today - 2.0 * DAY_S + HOUR, NOW, None),
        ("b", "Sword_5001", 500, 2, "[]", today + HOUR, today + HOUR, NOW + DAY_S, None),
    ]);

    let days = history.daily_medians("Sword_5001", 5, NOW, 1).unwrap();

    let summary: Vec<(f64, f64, usize)> = days.iter().map(|d| (d.day_start, d.median, d.listings)).collect();
    assert_eq!(
        summary,
        [
            (today - 2.0 * DAY_S, 200.0, 2), // 100 and 300
            (today - DAY_S, 300.0, 1),
            (today, 275.0, 2), // 300 and 250 a piece
        ]
    );
    assert_eq!(history.daily_medians("Sword_5001", 5, NOW, 2).unwrap().len(), 2);
}

#[test]
fn sales_are_tracked_once_a_crawl_of_the_rarity_ran_after_the_item_was_seen() {
    let (dir, history) = history(&[("a", "Sword_5001", 500, 1, "[]", NOW - DAY_S, NOW - DAY_S, NOW + DAY_S, None)]);
    let since = NOW - 2.0 * DAY_S;
    assert!(!history.sales_tracked("Sword_5001", since).unwrap());
    assert!(!history.sales_tracked("Axe_5001", since).unwrap());

    Connection::open(dir.path().join("market_history.sqlite"))
        .unwrap()
        .execute("INSERT INTO crawl_passes VALUES (5, ?1, ?2)", [NOW - HOUR, NOW - HOUR / 2.0])
        .unwrap();

    assert!(history.sales_tracked("Sword_5001", since).unwrap());
}

#[test]
fn open_listing_counts_per_item() {
    let (_dir, history) = history(&[
        ("a", "Sword_4001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + DAY_S, None),
        ("b", "Sword_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + DAY_S, None),
        ("c", "Sword_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + DAY_S, None),
        ("gone", "Sword_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + DAY_S, Some(NOW - 1.0)),
        ("other", "Axe_5001", 100, 1, "[]", NOW - HOUR, NOW - HOUR, NOW + DAY_S, None),
    ]);

    let counts = history.open_listing_counts(&["Sword_4001", "Sword_5001", "Sword_6001"], NOW, DAY_S).unwrap();

    assert_eq!(counts.get("Sword_4001"), Some(&1));
    assert_eq!(counts.get("Sword_5001"), Some(&2));
    assert_eq!(counts.get("Sword_6001"), None);
    assert_eq!(counts.len(), 2);
}

#[test]
fn opening_a_new_file_creates_an_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let history = MarketHistory::open(&dir.path().join("new.sqlite")).unwrap();

    assert!(history.open_listings("Sword_5001", NOW, DAY_S).unwrap().is_empty());
}
