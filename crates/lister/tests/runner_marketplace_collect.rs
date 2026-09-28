//! Port of DnDTools' `tests/test_marketplace_runner.py`, part 3: collecting payouts
//! (`collect_payouts`).

#[path = "runner_fakes.rs"]
mod fakes;

use std::sync::Arc;

use fakes::*;
use lister::job::{CancelToken, RunReport, Runner};
use lister::marketplace_state::{ListingsSnapshot, FIRST_PAGE};
use lister::runner::marketplace::listing_pages;
use lister::runner::SafetyCheck;

const HELM: (i64, i64, &str, i64) = (1, 3, "GreatHelm_3001", 200);
const ROBE: (i64, i64, &str, i64) = (3, 2, "OracleRobe_4001", 690);

fn collect<G>(driver: &Arc<fakes::FakeDriver>, game: &Arc<G>, safety: Option<Arc<dyn SafetyCheck>>) -> RunReport
where
    G: lister::runner::MarketplaceGame + ClickListener + 'static,
{
    market_runner(driver, game, &CancelToken::new(), safety).collect_payouts(&mut |_| {})
}

fn listings(state: i64, prefix: &str, count: usize) -> Vec<(i64, String, i64)> {
    (0..count).map(|i| (state, format!("{prefix}{i}"), 100)).collect()
}

#[test]
fn collect_payouts_transfers_each_sold_listing() {
    let game = PayoutGame::new(
        vec![
            payout_snapshot(vec![HELM, ROBE]),
            payout_snapshot(vec![HELM, ROBE]),
            payout_snapshot(vec![(2, 2, "OracleRobe_4001", 690)]),
            payout_snapshot(vec![]),
        ],
        vec![1, 1],
    );
    let driver = FakeDriver::new();
    let report = collect(&driver, &game, None);
    // My Listings is re-opened and confirmed before every transfer: rows shift after each one.
    let mut expected = verify_clicks();
    expected.extend([layout().spot_row(1), point("transfer_all_button")]);
    expected.extend(verify_clicks());
    expected.extend([layout().spot_row(2), point("transfer_all_button")]);
    expected.extend(verify_clicks());
    assert_eq!(driver.clicks(), expected);
    let results: Vec<_> = report.results.iter().map(|r| (r.name.as_str(), r.status.as_str(), r.message.as_str())).collect();
    assert_eq!(
        results,
        [("GreatHelm_3001", "collected", "200g collected"), ("OracleRobe_4001", "collected", "expired item returned")]
    );
    assert_eq!(report.stopped_reason, None);
}

#[test]
fn collect_payouts_stops_on_transfer_failure() {
    let game = PayoutGame::new(vec![payout_snapshot(vec![HELM]), payout_snapshot(vec![HELM])], vec![658]);
    let report = collect(&FakeDriver::new(), &game, None);
    assert_eq!(report.results[0].status, "failed");
    let reason = report.stopped_reason.unwrap();
    assert!(reason.contains("Marketplace error 658") || reason.to_lowercase().contains("space"));
}

#[test]
fn collect_stops_when_the_mouse_moves_between_transfers() {
    let snapshots = vec![payout_snapshot(vec![HELM, ROBE]), payout_snapshot(vec![HELM, ROBE]), payout_snapshot(vec![ROBE])];
    let game = PayoutGame::new(snapshots, vec![1, 1]);
    let report = collect(&FakeDriver::new(), &game, Some(FakeSafety::counting(1, "mouse_interference")));
    assert_eq!(statuses(&report), ["collected"]);
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the mouse was moved"));
}

#[test]
fn collect_stops_when_the_mouse_is_taken_during_a_transfer() {
    let driver = FakeDriver::new();
    let snapshots = vec![payout_snapshot(vec![HELM, ROBE]), payout_snapshot(vec![HELM, ROBE]), payout_snapshot(vec![ROBE])];
    let game = PayoutGame::new(snapshots, vec![1, 1]);
    let player = Arc::clone(&driver);
    game.during_transfer(move || player.set_pos((5, 5)));
    let report = collect(&driver, &game, None);
    assert_eq!(statuses(&report), ["collected"]);
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the mouse was moved"));
}

#[test]
fn collect_payouts_checks_every_page_with_listings() {
    // Verified in game: My Listings only reports the page on screen, so a sale on page 2 was missed.
    let mut shown = listings(1, "Item", 10);
    shown.extend([(1, "Bolt_2001".to_string(), 80), (3, "ArcaneEssence_3001".to_string(), 57)]);
    let game = PagedListingsGame::new(shown, 0);
    let report = collect(&FakeDriver::new(), &game, None);
    let results: Vec<_> = report.results.iter().map(|r| (r.name.as_str(), r.message.as_str())).collect();
    assert_eq!(results, [("ArcaneEssence_3001", "57g collected")]);
    assert_eq!(report.stopped_reason, None);
    assert!(game.listings().iter().all(|(state, _, _)| *state != 3));
}

#[test]
fn collect_payouts_on_several_pages_in_one_run() {
    let mut shown = listings(1, "Item", 3);
    shown.push((3, "GemRing_4001".to_string(), 57));
    shown.extend(listings(1, "More", 8));
    shown.push((3, "ArcaneEssence_3001".to_string(), 57));
    // The game reopens on the page last used.
    let game = PagedListingsGame::new(shown, 1);
    let report = collect(&FakeDriver::new(), &game, None);
    let mut names: Vec<&str> = report.results.iter().map(|r| r.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["ArcaneEssence_3001", "GemRing_4001"]);
    assert_eq!(report.stopped_reason, None);
    assert_eq!(game.listings().len(), 11);
}

#[test]
fn collect_payouts_with_nothing_sold_reads_each_page_once() {
    let game = PagedListingsGame::new(listings(1, "Item", 15), 0);
    let driver = FakeDriver::new();
    let report = collect(&driver, &game, None);
    assert!(report.results.is_empty());
    assert_eq!(report.stopped_reason, None);
    assert_eq!(driver.count(&Action::Click(point("next_page_arrow"))), 1);
}

#[test]
fn listing_pages_are_the_pages_before_the_first_free_spot() {
    let snapshot = |available: Vec<i64>| ListingsSnapshot { received_at: 0.0, available, current_page: FIRST_PAGE, payouts: vec![] };
    assert_eq!(listing_pages(&snapshot(vec![])), 0..4);
    assert_eq!(listing_pages(&snapshot(vec![0, 1])), 0..0);
    assert_eq!(listing_pages(&snapshot(vec![10, 30])), 0..1);
    assert_eq!(listing_pages(&snapshot(vec![12, 5, 40])), 0..1);
    assert_eq!(listing_pages(&snapshot(vec![11])), 0..2);
}
