//! Port of DnDTools' `tests/test_marketplace_runner.py`, part 2: reading the market (`price_all`,
//! `crawl_market`).

#[path = "runner_fakes.rs"]
mod fakes;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use fakes::*;
use input::marketplace::tab_icon_index;
use lister::job::{CancelToken, RunReport, Runner};
use lister::marketplace_state::MarketplaceState;
use lister::plan::{MarketBucket, PlanEntry};
use lister::runner::MarketplaceRunner;
use market::MarketRow;

fn inv(uid: &str) -> PlanEntry {
    entry(uid, "2", 0, 900)
}

fn plain(driver: &Arc<fakes::FakeDriver>, game: &Arc<ScriptedGame>) -> MarketplaceRunner {
    market_runner(driver, game, &CancelToken::new(), None)
}

fn price(runner: &MarketplaceRunner, entries: &[PlanEntry]) -> (HashMap<String, MarketBucket>, RunReport) {
    runner.price_all(entries, &mut |_| {})
}

/// `crawl_market(pages, rarities=(5,), gear_only=False)`.
fn crawl_epic(runner: &MarketplaceRunner, pages: u32) -> RunReport {
    runner.crawl_market_with(pages, &mut |_| {}, &[5], false, None)
}

/// A runner whose crawl passes are recorded into the returned log (`_crawl_runner`); the observer
/// says 7 listings are gone.
fn crawl_runner(game: &Arc<ScriptedGame>) -> (MarketplaceRunner, Arc<Mutex<Vec<i32>>>) {
    let passes = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&passes);
    let runner = plain(&FakeDriver::new(), game).with_pass_observer(Some(Arc::new(move |rarity: i32, _started: f64| {
        log.lock().unwrap().push(rarity);
        Some(7)
    })));
    (runner, passes)
}

fn gold_band_page() -> Vec<MarketRow> {
    (0..10).map(|i| row("GoldBand_3001", 100 + i, "")).collect()
}

fn heater_shields(from: i64, count: i64) -> Vec<MarketRow> {
    (0..count).map(|i| row("HeaterShield_5001", from + i, "")).collect()
}

#[test]
fn crawl_gear_only_ticks_every_class_and_stops_at_known_listings() {
    let driver = FakeDriver::new();
    let full = full_page();
    let game = Script { searches: searches(vec![Some(full.clone()); 3]), ..script(vec![2]) }.build();
    let known = full.clone();
    let is_old = move |rows: &[MarketRow]| rows == known.as_slice();
    let report = plain(&driver, &game).crawl_market_with(10, &mut |_| {}, &[5], true, Some(&is_old));
    // The dropdown closes after each tick, so it is reopened every time.
    let expected: Vec<_> = (0..10).flat_map(|i| [point("class_dropdown"), layout().class_option(i)]).collect();
    let start = verify_clicks().len() + 4;
    assert_eq!(driver.clicks()[start..start + 20], expected[..]);
    // The first page was already known: stop.
    assert_eq!(report.results[0].message, "1 pages, 10 listings");
}

#[test]
fn price_all_runs_the_search_flow_per_item() {
    let driver = FakeDriver::new();
    let shield = row("HeaterShield_5001", 300, "");
    // A same-roll then an all-roll search per item.
    let game = Script { searches: searches(vec![Some(vec![]), Some(vec![shield.clone()]), None, None]), ..script(vec![2, 3]) }.build();
    let entries = [entry("a", "20", 13, 900), entry("b", "2", 1, 900)];
    let (progress, mut on_progress) = progress_log();
    let (rows, report) = plain(&driver, &game).price_all(&entries, &mut on_progress);
    let icon = tab_icon_index("20", &MAPPING).unwrap() as i32;
    let mut expected = verify_clicks();
    expected.extend([
        layout().spot_row(2),
        layout().tab_icon(icon),
        layout().item_centre("20", 13, 1, 1),
        point("form_search_button"),
        point("market_attr_reset"),
        point("market_search_button"),
        // Back, and confirmed, before the next item.
        point("view_market_tab"),
        point("my_listings_tab"),
    ]);
    assert_eq!(driver.clicks()[..10], expected[..]);
    assert!(!driver.clicked(point("create_listing_button")));
    // Item b's searches never answered: its (empty) view is marked incomplete.
    let expected_rows = HashMap::from([
        ("a".to_string(), MarketBucket { same: vec![], all: vec![shield], degraded: false }),
        ("b".to_string(), MarketBucket { same: vec![], all: vec![], degraded: true }),
    ]);
    assert_eq!(rows, expected_rows);
    assert_eq!(statuses(&report), ["priced", "no_results"]);
    assert!(report.results[1].message.contains("search incomplete"));
    assert_eq!(progress.lock().unwrap().len(), 2);
    assert_eq!(report.stopped_reason, None);
}

#[test]
fn price_all_refuses_without_listings_snapshot() {
    let runner = market_runner(&FakeDriver::new(), &Arc::new(MarketplaceState::default()), &CancelToken::new(), None);
    let (rows, report) = price(&runner, &[inv("a")]);
    assert!(rows.is_empty());
    assert!(report.stopped_reason.unwrap().contains("My Listings"));
}

#[test]
fn price_all_never_opens_the_locked_seasonal_stash() {
    let driver = FakeDriver::new();
    let game = Script { searches: searches(vec![Some(vec![]); 2]), ..script(vec![2]) }.build();
    let (rows, report) = price(&plain(&driver, &game), &[entry("a", "30", 0, 900)]);
    assert!(rows.is_empty());
    assert_eq!(report.stopped_reason.as_deref(), Some("Item a is in the locked Seasonal Shared Stash, which is never touched."));
    assert!(driver.actions().is_empty());
}

#[test]
fn price_all_reads_up_to_three_pages_of_all_roll_results() {
    let driver = FakeDriver::new();
    let pages = vec![Some(vec![]), Some(heater_shields(300, 10)), Some(heater_shields(400, 10)), Some(heater_shields(500, 3))];
    let game = Script { searches: searches(pages), ..script(vec![2]) }.build();
    let (rows, _) = price(&plain(&driver, &game), &[inv("a")]);
    // Stops once a page is short.
    assert_eq!(driver.count(&Action::Click(point("market_next_page"))), 2);
    assert_eq!(rows["a"].all.len(), 23);
}

#[test]
fn crawl_stops_when_the_mouse_is_taken() {
    // The player grabs the mouse right after the first result page arrives.
    let driver = FakeDriver::new();
    let player = Arc::clone(&driver);
    let game = Script {
        searches: searches(vec![Some(full_page()); 3]),
        after_item_list: Some(Arc::new(move || player.set_pos((5, 5)))),
        ..script(vec![2])
    }
    .build();
    let report = crawl_epic(&plain(&driver, &game), 5);
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the mouse was moved"));
    assert!(!driver.clicked(point("market_next_page")));
}

#[test]
fn crawl_stops_for_safety_between_pages() {
    let game = Script { searches: searches(vec![Some(full_page()); 3]), ..script(vec![2]) }.build();
    let safety = FakeSafety::counting(1, "mouse_interference");
    let runner = market_runner(&FakeDriver::new(), &game, &CancelToken::new(), Some(safety));
    let report = crawl_epic(&runner, 5);
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the mouse was moved"));
}

#[test]
fn crawl_market_reads_each_rarity_newest_first() {
    let driver = FakeDriver::new();
    let full = full_page();
    let game = Script { searches: searches(vec![Some(full.clone()), Some(full.clone()), Some(full[..4].to_vec())]), ..script(vec![2]) }.build();
    let report = crawl_epic(&plain(&driver, &game), 5);
    let clicks = driver.clicks()[verify_clicks().len()..].to_vec();
    let expected = [
        point("view_market_tab"),
        point("market_reset_filters"),
        point("rarity_dropdown"),
        layout().rarity_option(5),
        point("market_search_button"),
    ];
    assert_eq!(clicks[..5], expected);
    // Stops after the short third page.
    assert_eq!(clicks.iter().filter(|c| **c == point("market_next_page")).count(), 2);
    assert_eq!(clicks.last(), Some(&point("my_listings_tab")));
    assert_eq!((report.results[0].name.as_str(), report.results[0].message.as_str()), ("Epic", "3 pages, 24 listings"));
}

#[test]
fn next_page_tries_arrow_positions_until_one_works() {
    let driver = FakeDriver::new();
    let full = full_page();
    let pages = vec![Some(full.clone()), None, None, Some(full.clone()), Some(full[..2].to_vec())];
    let game = Script { searches: searches(pages), ..script(vec![2]) }.build();
    crawl_epic(&plain(&driver, &game), 5);
    let arrow_y = point("market_next_page").1;
    let tried: Vec<_> = driver.clicks().into_iter().filter(|c| c.1 == arrow_y).collect();
    let candidate = |attempt| layout().next_page_candidate(attempt);
    // Remembers the one that worked.
    assert_eq!(tried, [candidate(0), candidate(1), candidate(2), candidate(2)]);
}

#[test]
fn price_all_reports_each_scan_to_the_observer() {
    let scans = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&scans);
    let game = Script { searches: searches(vec![Some(vec![]), Some(heater_shields(300, 4))]), ..script(vec![2]) }.build();
    let runner = plain(&FakeDriver::new(), &game).with_scan_observer(Some(Arc::new(
        move |item_id: &str, _started: f64, rows: &[MarketRow], complete: bool| {
            log.lock().unwrap().push((item_id.to_string(), rows.len(), complete));
        },
    )));
    price(&runner, &[entry("a", "20", 0, 900)]);
    // The entry has no item id; a short page means every listing was seen.
    assert_eq!(*scans.lock().unwrap(), [(String::new(), 4, true)]);
}

#[test]
fn crawl_read_to_its_last_page_reports_a_complete_pass() {
    let full = full_page();
    let game = Script { searches: searches(vec![Some(full.clone()), Some(full.clone()), Some(full[..4].to_vec())]), ..script(vec![2]) }.build();
    let (runner, passes) = crawl_runner(&game);
    let report = crawl_epic(&runner, 50);
    assert_eq!(*passes.lock().unwrap(), [5]);
    assert_eq!(report.results[0].message, "3 pages, 24 listings; 7 gone since the last full crawl (likely sold)");
}

#[test]
fn limited_or_incremental_crawls_are_not_complete_passes() {
    let full = full_page();
    let limited = Script { searches: searches(vec![Some(full.clone()), Some(full.clone()), Some(full[..4].to_vec())]), ..script(vec![2]) }.build();
    let (runner, limited_passes) = crawl_runner(&limited);
    crawl_epic(&runner, 2);
    let incremental = Script { searches: searches(vec![Some(full.clone()), Some(full[..4].to_vec())]), ..script(vec![2]) }.build();
    let (runner, incremental_passes) = crawl_runner(&incremental);
    let never_old = |_: &[MarketRow]| false;
    runner.crawl_market_with(50, &mut |_| {}, &[5], false, Some(&never_old));
    assert!(limited_passes.lock().unwrap().is_empty());
    assert!(incremental_passes.lock().unwrap().is_empty());
}

#[test]
fn search_stops_at_the_last_page_the_game_reports() {
    let driver = FakeDriver::new();
    let page = gold_band_page();
    // Counting from 1: page 2 of 2.
    let game = Script {
        searches: searches(vec![Some(vec![]), Some(page.clone()), Some(page)]),
        page_numbers: [(1, 2), (2, 2)].into(),
        ..script(vec![2])
    }
    .build();
    let (rows, _) = price(&plain(&driver, &game), &[inv("a")]);
    assert_eq!(rows["a"].all.len(), 20);
    assert!(!rows["a"].degraded);
    // No click past the end.
    assert_eq!(driver.count(&Action::Click(point("market_next_page"))), 1);
}

#[test]
fn a_full_last_page_is_the_end_not_a_failed_search() {
    let page = gold_band_page();
    // Counting from 0: the last page is 1.
    let game = Script {
        searches: searches(vec![Some(vec![]), Some(page.clone()), Some(page)]),
        page_numbers: [(0, 2), (1, 2)].into(),
        ..script(vec![2])
    }
    .build();
    let (rows, report) = price(&plain(&FakeDriver::new(), &game), &[inv("a")]);
    assert_eq!(rows["a"].all.len(), 20);
    assert!(!rows["a"].degraded);
    assert!(!report.results[0].message.contains("incomplete"));
}

#[test]
fn a_failed_page_turn_mid_results_is_still_incomplete() {
    let game = Script { searches: searches(vec![Some(vec![]), Some(gold_band_page())]), page_numbers: [(0, 5)].into(), ..script(vec![2]) }.build();
    let (rows, _) = price(&plain(&FakeDriver::new(), &game), &[inv("a")]);
    assert!(rows["a"].degraded);
}

#[test]
fn crawl_ending_on_a_full_last_page_is_a_complete_pass() {
    let game = Script { searches: searches(vec![Some(full_page()); 2]), page_numbers: [(1, 2), (2, 2)].into(), ..script(vec![2]) }.build();
    let (runner, passes) = crawl_runner(&game);
    crawl_epic(&runner, 50);
    assert_eq!(*passes.lock().unwrap(), [5]);
}

#[test]
fn a_search_that_never_opened_still_returns_to_a_confirmed_my_listings() {
    // No results ever arrive (e.g. an item the market won't search): we are still on My Listings,
    // where clicking its own tab would not make the game resend the list.
    let driver = FakeDriver::new();
    let game = Script { searches: searches(vec![None, None]), ..script(vec![2]) }.build();
    let (rows, report) = price(&plain(&driver, &game), &[inv("a")]);
    assert_eq!(report.stopped_reason, None);
    assert!(rows["a"].degraded);
    let clicks = driver.clicks();
    assert_eq!(clicks[clicks.len() - 2..], verify_clicks()[..]);
}
