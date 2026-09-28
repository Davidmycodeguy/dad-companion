//! Port of DnDTools' `tests/test_marketplace_runner.py`, part 1: listing (`run`).

#[path = "runner_fakes.rs"]
mod fakes;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fakes::*;
use lister::job::{CancelToken, Reprice, RunReport, Runner};
use lister::marketplace_state::{ListingsSnapshot, MarketplaceState, RegisterOutcome, FIRST_PAGE, MAX_SNAPSHOT_AGE_S};
use lister::plan::PlanEntry;
use lister::runner::marketplace::{DEFAULT_SKIP_NOTE, NOT_ON_MY_LISTINGS};
use lister::runner::{InputError, MarketplaceRunner, CURSOR_DEVIATION_PX};

/// An inventory entry at `slot` priced 900.
fn inv(uid: &str, slot: i64) -> PlanEntry {
    entry(uid, "2", slot, 900)
}

fn run(runner: &MarketplaceRunner, entries: &[PlanEntry]) -> RunReport {
    runner.run(entries, false, &mut |_| {}, None)
}

fn run_with(runner: &MarketplaceRunner, entries: &[PlanEntry], reprice: &dyn Reprice) -> RunReport {
    runner.run(entries, false, &mut |_| {}, Some(reprice))
}

fn plain(driver: &Arc<fakes::FakeDriver>, game: &Arc<ScriptedGame>) -> MarketplaceRunner {
    market_runner(driver, game, &CancelToken::new(), None)
}

fn cancel_on_click(driver: &Arc<fakes::FakeDriver>, cancel: &CancelToken, key: &'static str) {
    let cancel = cancel.clone();
    driver.on_action(move |action, _| {
        if *action == Action::Click(point(key)) {
            cancel.cancel();
        }
    });
}

#[test]
fn run_clicks_full_sequence_for_one_item() {
    let driver = FakeDriver::new();
    let report = run(&plain(&driver, &script(used(2)).build()), &[entry("a", "4", 13, 900)]);
    assert_eq!(report.stopped_reason, None);
    assert_eq!(statuses(&report), ["listed"]);
    let mut expected: Vec<Action> = verify_clicks().into_iter().map(Action::Click).collect();
    expected.extend([
        Action::Click(layout().spot_row(2)),
        Action::Click(layout().tab_icon(1)),
        Action::Click(layout().item_centre("4", 13, 1, 1)),
        Action::Click(point("price_field")),
        Action::Type("900".into()),
        Action::Click(point("create_listing_button")),
        // "Would you like to list the item?" -> Yes
        Action::Click(point("confirm_listing_yes")),
    ]);
    assert_eq!(driver.actions(), expected);
}

#[test]
fn run_turns_pages_when_spots_full() {
    let driver = FakeDriver::new();
    run(&plain(&driver, &script(used(19)).build()), &[inv("a", 0), inv("b", 1)]);
    let clicks = driver.clicks()[verify_clicks().len()..].to_vec();
    let arrow = point("next_page_arrow");
    // Index 19 is page 1, row 9; index 20 is page 2, row 0.
    assert_eq!(clicks[..2], [arrow, layout().spot_row(9)]);
    assert_eq!(clicks.iter().filter(|c| **c == arrow).count(), 2);
    let second_arrow = (1..clicks.len()).find(|&i| clicks[i] == arrow).unwrap();
    assert_eq!(clicks[second_arrow + 1], layout().spot_row(0));
}

#[test]
fn dry_run_never_clicks_create_listing() {
    let driver = FakeDriver::new();
    let runner = plain(&driver, &script(used(0)).build());
    let report = runner.run(&[inv("a", 0), inv("b", 1)], true, &mut |_| {}, None);
    assert_eq!(statuses(&report), ["dry_run", "dry_run"]);
    assert!(!driver.clicked(point("create_listing_button")));
    // The second item uses the next spot.
    assert!(driver.clicked(layout().spot_row(1)));
}

#[test]
fn run_refuses_without_listings_snapshot() {
    let driver = FakeDriver::new();
    let runner = market_runner(&driver, &Arc::new(MarketplaceState::default()), &CancelToken::new(), None);
    let report = run(&runner, &[inv("a", 0)]);
    assert!(report.results.is_empty());
    assert!(report.stopped_reason.unwrap().contains("My Listings"));
    assert!(driver.actions().is_empty());
}

#[test]
fn run_stops_on_timeout_and_general_failure() {
    // A timeout leaves the item "unconfirmed", and No is clicked in case the dialog is still open.
    let driver = FakeDriver::new();
    let game = Script { outcomes: [RegisterOutcome::Timeout].into(), ..script(used(2)) }.build();
    let report = run(&plain(&driver, &game), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["unconfirmed"]);
    assert!(report.stopped_reason.unwrap().to_lowercase().contains("not confirmed"));
    assert_eq!(driver.actions().last(), Some(&Action::Click(point("confirm_listing_no"))));

    // A general failure (657) leaves it "failed".
    let game = Script { outcomes: [RegisterOutcome::Failed(657)].into(), ..script(used(2)) }.build();
    let report = run(&plain(&FakeDriver::new(), &game), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["failed"]);
    assert!(report.stopped_reason.unwrap().to_lowercase().contains("gold"));
}

#[test]
fn run_continues_after_item_level_failure() {
    let driver = FakeDriver::new();
    let game = Script { outcomes: [RegisterOutcome::Failed(666), RegisterOutcome::Ok].into(), ..script(used(2)) }.build();
    let report = run(&plain(&driver, &game), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["failed", "listed"]);
    assert_eq!(report.stopped_reason, None);
    let clicks = driver.clicks();
    let yes = clicks.iter().position(|c| *c == point("confirm_listing_yes")).unwrap();
    // Back on a confirmed My Listings before the next item.
    assert_eq!(clicks[yes + 1..yes + 3], verify_clicks()[..]);
}

#[test]
fn item_level_failure_stops_when_my_listings_cannot_be_confirmed() {
    let game = Script { outcomes: [RegisterOutcome::Failed(666)].into(), ..script(used(2)) }.build();
    let driver = FakeDriver::new();
    let runner = plain(&driver, &game);
    // E.g. an error popup covers the tabs.
    let popup = Arc::clone(&game);
    driver.on_action(move |action, _| {
        if *action == Action::Click(point("create_listing_button")) {
            popup.set_responsive(false);
        }
    });
    let report = run(&runner, &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["failed"]);
    assert_eq!(report.stopped_reason.as_deref(), Some(NOT_ON_MY_LISTINGS));
}

#[test]
fn page_turn_the_game_does_not_confirm_stops_before_the_spot_click() {
    let driver = FakeDriver::new();
    let game = Script { turns_pages: false, ..script(used(19)) }.build();
    let report = run(&plain(&driver, &game), &[inv("a", 0)]);
    assert!(report.results.is_empty());
    assert!(report.stopped_reason.unwrap().contains("Couldn't turn My Listings to page 2"));
    assert!(!driver.clicked(layout().spot_row(9)));
}

#[test]
fn cancel_while_the_game_is_unfocused_does_not_click_no_blindly() {
    let driver = FakeDriver::new();
    let cancel = CancelToken::new();
    let safety = FakeSafety::recording(&driver, Some("game_window_unfocused"), true);
    let runner = market_runner(&driver, &script(used(0)).build(), &cancel, Some(safety));
    cancel_on_click(&driver, &cancel, "create_listing_button");
    let report = run(&runner, &[inv("a", 0)]);
    assert!(!driver.clicked(point("confirm_listing_no")));
    assert!(report.stopped_reason.unwrap().contains("may still be open"));
}

#[test]
fn run_stops_when_listing_not_confirmed() {
    let game = Script { confirm: false, ..script(used(2)) }.build();
    let report = run(&plain(&FakeDriver::new(), &game), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["unconfirmed"]);
    assert!(report.stopped_reason.unwrap().contains("couldn't confirm"));
}

#[test]
fn run_stops_when_cancelled() {
    // Cancel lands after Create Listing but before the confirmation: Yes is never clicked, so
    // nothing is listed and no fee is charged.
    let driver = FakeDriver::new();
    let cancel = CancelToken::new();
    let runner = market_runner(&driver, &script(used(2)).build(), &cancel, None);
    cancel_on_click(&driver, &cancel, "create_listing_button");
    let report = run(&runner, &[inv("a", 0), inv("b", 1)]);
    assert!(report.results.is_empty());
    assert_eq!(report.stopped_reason.as_deref(), Some("Cancelled"));
    assert!(!driver.clicked(point("confirm_listing_yes")));
    // The dialog is dismissed.
    assert_eq!(driver.actions().last(), Some(&Action::Click(point("confirm_listing_no"))));
}

#[test]
fn run_cancel_between_items_keeps_the_listed_one() {
    let driver = FakeDriver::new();
    let cancel = CancelToken::new();
    let runner = market_runner(&driver, &script(used(2)).build(), &cancel, None);
    cancel_on_click(&driver, &cancel, "confirm_listing_yes");
    let report = run(&runner, &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["listed"]);
    assert_eq!(report.stopped_reason.as_deref(), Some("Cancelled"));
}

#[test]
fn run_stops_when_no_free_spots() {
    let driver = FakeDriver::new();
    let report = run(&plain(&driver, &script(used(40)).build()), &[inv("a", 0)]);
    assert!(report.stopped_reason.unwrap().contains("No free listing spots"));
    assert!(report.results.is_empty());
    assert!(driver.actions().is_empty());
}

#[test]
fn run_stops_when_driver_click_raises() {
    let driver = FakeDriver::new();
    let clicks = AtomicUsize::new(0);
    // My Listings check (2 clicks) + the first item (6 clicks), then it fails.
    driver.fail_when(move |action| {
        let failing = matches!(action, Action::Click(_)) && clicks.fetch_add(1, Ordering::SeqCst) + 1 > 8;
        failing.then(|| InputError::new("Driver crashed"))
    });
    let game = Script { outcomes: [RegisterOutcome::Ok, RegisterOutcome::Ok].into(), ..script(used(2)) }.build();
    let (progress, mut on_progress) = progress_log();
    let report = plain(&driver, &game).run(&[inv("a", 0), inv("b", 1)], false, &mut on_progress, None);
    assert_eq!(statuses(&report), ["listed", "failed"]);
    let failed = &report.results[1];
    assert_eq!((failed.unique_id.as_str(), failed.name.as_str(), failed.message.as_str()), ("b", "Item b", "Driver crashed"));
    assert!(report.stopped_reason.unwrap().contains("Stopped:"));
    let reported: Vec<String> = progress.lock().unwrap().iter().map(|r| r.unique_id.clone()).collect();
    assert_eq!(reported, ["a", "b"]);
}

#[test]
fn run_stops_for_safety_checkpoint_false() {
    let driver = FakeDriver::new();
    let runner = market_runner(&driver, &script(used(0)).build(), &CancelToken::new(), Some(FakeSafety::failing("game not focused")));
    let report = run(&runner, &[inv("a", 0)]);
    assert!(report.results.is_empty());
    assert!(report.stopped_reason.unwrap().contains("Stopped for safety: game not focused"));
    assert!(driver.actions().is_empty());
}

#[test]
fn run_stops_on_unmapped_stash_tab() {
    let driver = FakeDriver::new();
    // Stash "99" is not in MAPPING.
    let report = run(&plain(&driver, &script(used(0)).build()), &[entry("a", "99", 0, 900)]);
    assert!(report.results.is_empty());
    assert!(report.stopped_reason.unwrap().contains("not mapped"));
    assert!(driver.actions().is_empty());
}

#[test]
fn run_validates_every_tab_mapping_before_any_click() {
    let driver = FakeDriver::new();
    let report = run(&plain(&driver, &script(used(0)).build()), &[inv("a", 0), entry("b", "99", 1, 900)]);
    assert!(report.results.is_empty());
    assert_eq!(report.stopped_reason.as_deref(), Some("Stash tab for Item b is not mapped in DnDTools settings."));
    assert!(driver.actions().is_empty());
}

#[test]
fn run_never_opens_the_locked_seasonal_stash() {
    // Stash 30 has a tab icon (it is in MAPPING), but the lister must never open it.
    for entries in [vec![entry("a", "30", 0, 900)], vec![inv("b", 0), entry("a", "30", 1, 900)]] {
        let driver = FakeDriver::new();
        let report = run(&plain(&driver, &script(used(0)).build()), &entries);
        assert!(report.results.is_empty());
        assert_eq!(
            report.stopped_reason.as_deref(),
            Some("Item a is in the locked Seasonal Shared Stash, which is never touched.")
        );
        assert!(driver.actions().is_empty());
    }
}

#[test]
fn run_continues_on_fail_code_662() {
    let game = Script { outcomes: [RegisterOutcome::Failed(662), RegisterOutcome::Ok].into(), ..script(used(2)) }.build();
    let report = run(&plain(&FakeDriver::new(), &game), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["failed", "listed"]);
    assert_eq!(report.stopped_reason, None);
}

#[test]
fn run_uses_free_spots_from_the_game_in_order() {
    // Spots 0-4 taken except a gap at 2 (e.g. something sold): fill 2, then 5.
    let driver = FakeDriver::new();
    let report = run(&plain(&driver, &script(vec![2, 5, 6]).build()), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["listed", "listed"]);
    let (two, five) = (layout().spot_row(2), layout().spot_row(5));
    let spots: Vec<_> = driver.clicks().into_iter().filter(|c| *c == two || *c == five).collect();
    assert_eq!(spots, [two, five]);
}

#[test]
fn run_stops_when_free_spots_run_out() {
    let report = run(&plain(&FakeDriver::new(), &script(vec![38]).build()), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["listed"]);
    assert_eq!(report.stopped_reason.as_deref(), Some("No free listing spots left."));
}

#[test]
fn run_calls_on_progress_for_failed_items() {
    let game = Script { outcomes: [RegisterOutcome::Failed(657)].into(), ..script(used(2)) }.build();
    let (progress, mut on_progress) = progress_log();
    plain(&FakeDriver::new(), &game).run(&[inv("a", 0), inv("b", 1)], false, &mut on_progress, None);
    let progress = progress.lock().unwrap();
    assert_eq!(progress.len(), 1);
    assert_eq!((progress[0].status.as_str(), progress[0].unique_id.as_str()), ("failed", "a"));
}

#[test]
fn run_refuses_when_my_listings_was_seen_long_ago() {
    // Seen too long ago: the player may be anywhere, so nothing is clicked.
    let game = script(used(0)).build();
    game.set_now(1000.0 + MAX_SNAPSHOT_AGE_S + 1.0);
    let driver = FakeDriver::new();
    let report = run(&plain(&driver, &game), &[inv("a", 0)]);
    assert!(report.results.is_empty());
    assert_eq!(report.stopped_reason.as_deref(), Some("Open (or re-open) Trade → Marketplace → My Listings in the game first."));
    assert!(driver.actions().is_empty());
}

#[test]
fn run_stops_when_the_game_does_not_confirm_my_listings() {
    let driver = FakeDriver::new();
    let game = Script { responsive: false, ..script(used(0)) }.build();
    let report = run(&plain(&driver, &game), &[inv("a", 0)]);
    assert!(report.results.is_empty());
    assert_eq!(report.stopped_reason.as_deref(), Some(NOT_ON_MY_LISTINGS));
    // Nothing after the unanswered check.
    assert_eq!(driver.clicks(), verify_clicks());
    assert_eq!(driver.actions().len(), verify_clicks().len());
}

#[test]
fn my_listings_on_another_page_is_tracked_and_walked_back() {
    // The game reopens My Listings on the page last used (here page 2); the free spot is on page 1.
    let driver = FakeDriver::new();
    let game = Script { answers_page: FIRST_PAGE + 1, ..script(vec![3]) }.build();
    let report = run(&plain(&driver, &game), &[inv("a", 0)]);
    assert_eq!(statuses(&report), ["listed"]);
    assert_eq!(driver.clicks()[2..4], [point("prev_page_arrow"), layout().spot_row(3)]);
}

#[test]
fn going_back_a_page_uses_the_previous_page_arrow() {
    // After the first listing the game shows page 3; the next free spot is on page 2.
    let driver = FakeDriver::new();
    let game = Script { after_listing_page: Some(FIRST_PAGE + 2), ..script(vec![3, 12]) }.build();
    let report = run(&plain(&driver, &game), &[inv("a", 0), inv("b", 1)]);
    assert_eq!(statuses(&report), ["listed", "listed"]);
    let clicks = driver.clicks();
    let yes = clicks.iter().position(|c| *c == point("confirm_listing_yes")).unwrap();
    assert_eq!(clicks[yes + 1..yes + 3], [point("prev_page_arrow"), layout().spot_row(2)]);
}

#[test]
fn a_search_that_returns_to_a_later_page_keeps_going() {
    // Returning from a market search, the game shows the page of the spot the form was opened on.
    let game = Script {
        searches: searches(vec![Some(vec![]), Some(vec![]), Some(vec![]), Some(vec![])]),
        answers_page: FIRST_PAGE + 1,
        ..script(vec![12, 13])
    }
    .build();
    let keep = Recheck(|entry: &PlanEntry, _: &_| decision(Some(entry.price), ""));
    let report = run_with(&plain(&FakeDriver::new(), &game), &[inv("a", 0), inv("b", 1)], &keep);
    assert_eq!(statuses(&report), ["listed", "listed"]);
}

#[test]
fn run_accepts_recent_listings_snapshot() {
    let game = script(used(0)).build();
    game.set_now(1000.0 + MAX_SNAPSHOT_AGE_S - 1.0);
    let report = run(&plain(&FakeDriver::new(), &game), &[inv("a", 0)]);
    assert_eq!(report.stopped_reason, None);
    assert_eq!(statuses(&report), ["listed"]);
}

#[test]
fn run_reopens_my_listings_on_page_one_when_another_page_was_open() {
    let driver = FakeDriver::new();
    let base = script(used(0));
    let snapshot = ListingsSnapshot { current_page: FIRST_PAGE + 1, ..base.snapshot.clone() };
    let report = run(&plain(&driver, &Script { snapshot, ..base }.build()), &[inv("a", 0)]);
    assert_eq!(report.stopped_reason, None);
    assert_eq!(statuses(&report), ["listed"]);
    assert_eq!(driver.clicks()[..2], verify_clicks()[..]);
}

#[test]
fn run_accepts_first_page() {
    let report = run(&plain(&FakeDriver::new(), &script(used(0)).build()), &[inv("a", 0)]);
    assert_eq!(report.stopped_reason, None);
}

#[test]
fn snapshot_position_taken_after_create_click() {
    let driver = FakeDriver::new();
    let safety = FakeSafety::recording(&driver, None, true);
    run(&market_runner(&driver, &script(used(0)).build(), &CancelToken::new(), Some(safety)), &[inv("a", 0)]);
    let actions = driver.actions();
    let create = actions.iter().position(|a| *a == Action::Click(point("create_listing_button"))).unwrap();
    let snapshot = actions.iter().position(|a| *a == Action::Snapshot).unwrap();
    assert!(snapshot > create);
}

#[test]
fn snapshot_position_taken_after_dry_run_form_fill() {
    let driver = FakeDriver::new();
    let safety = FakeSafety::recording(&driver, None, true);
    let runner = market_runner(&driver, &script(used(0)).build(), &CancelToken::new(), Some(safety));
    runner.run(&[inv("a", 0)], true, &mut |_| {}, None);
    let actions = driver.actions();
    assert_eq!(actions.last(), Some(&Action::Snapshot));
    let typed = actions.iter().position(|a| *a == Action::Type("900".into())).unwrap();
    assert!(typed < actions.iter().position(|a| *a == Action::Snapshot).unwrap());
}

#[test]
fn mouse_moved_before_create_stops_without_clicking_create() {
    let driver = FakeDriver::new();
    let (fx, fy) = point("price_field");
    driver.on_action(move |action, driver| {
        if matches!(action, Action::Type(_)) {
            driver.set_pos((fx + CURSOR_DEVIATION_PX as i32 + 1, fy));
        }
    });
    let (progress, mut on_progress) = progress_log();
    let runner = plain(&driver, &script(used(0)).build());
    let report = runner.run(&[inv("a", 0), inv("b", 1)], false, &mut on_progress, None);
    assert!(!driver.clicked(point("create_listing_button")));
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: mouse moved during listing"));
    let results: Vec<_> = report.results.iter().map(|r| (r.unique_id.as_str(), r.status.as_str(), r.message.as_str())).collect();
    assert_eq!(results, [("a", "failed", "stopped before Create Listing — no fee charged")]);
    let reported: Vec<String> = progress.lock().unwrap().iter().map(|r| r.unique_id.clone()).collect();
    assert_eq!(reported, ["a"]);
}

#[test]
fn small_cursor_drift_does_not_stop() {
    let driver = FakeDriver::new();
    let (fx, fy) = point("price_field");
    let drift = CURSOR_DEVIATION_PX as i32;
    driver.on_action(move |action, driver| {
        if matches!(action, Action::Type(_)) {
            driver.set_pos((fx + drift, fy - drift));
        }
    });
    let report = run(&plain(&driver, &script(used(0)).build()), &[inv("a", 0)]);
    assert_eq!(report.stopped_reason, None);
    assert_eq!(statuses(&report), ["listed"]);
}

#[test]
fn safety_stop_uses_friendly_reason_text() {
    let driver = FakeDriver::new();
    let safety = FakeSafety::recording(&driver, Some("mouse_interference"), false);
    let report = run(&market_runner(&driver, &script(used(0)).build(), &CancelToken::new(), Some(safety)), &[inv("a", 0)]);
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the mouse was moved"));

    let driver = FakeDriver::new();
    let safety = FakeSafety::recording(&driver, Some("game_window_unfocused"), true);
    let cancelled = CancelToken::new();
    cancelled.cancel();
    let report = run(&market_runner(&driver, &script(used(0)).build(), &cancelled, Some(safety)), &[inv("a", 0)]);
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the game lost focus"));
}

#[test]
fn search_showing_another_item_stops_before_listing() {
    let driver = FakeDriver::new();
    let helm = PlanEntry { item_id: "HeaterShield_5001".into(), ..inv("a", 0) };
    let game = Script {
        searches: searches(vec![Some(vec![row("GreatHelm_5001", 300, "")]), Some(vec![])]),
        ..script(vec![2])
    }
    .build();
    let keep = Recheck(|entry: &PlanEntry, _: &_| decision(Some(entry.price), ""));
    let report = run_with(&plain(&driver, &game), &[helm], &keep);
    assert!(report.stopped_reason.unwrap().contains("instead of Item a"));
    assert!(!driver.clicked(point("create_listing_button")));
}

#[test]
fn recheck_lowers_price_when_the_market_dropped() {
    let driver = FakeDriver::new();
    let game = Script { searches: searches(vec![Some(vec![]), Some(vec![row("", 700, "")])]), ..script(vec![2, 3]) }.build();
    let lower = Recheck(|_: &PlanEntry, _: &_| decision(Some(630), ""));
    let report = run_with(&plain(&driver, &game), &[inv("a", 0)], &lower);
    assert!(driver.actions().contains(&Action::Type("630".into())));
    assert_eq!(report.results[0].message, "630g (market moved: planned 900g)");
}

#[test]
fn recheck_keeps_the_approved_price_when_market_rose() {
    let driver = FakeDriver::new();
    let game = Script { searches: searches(vec![Some(vec![]), Some(vec![])]), ..script(vec![2, 3]) }.build();
    let higher = Recheck(|_: &PlanEntry, _: &_| decision(Some(1200), ""));
    run_with(&plain(&driver, &game), &[inv("a", 0)], &higher);
    assert!(driver.actions().contains(&Action::Type("900".into())));
    assert!(!driver.actions().contains(&Action::Type("1200".into())));
}

#[test]
fn recheck_skips_items_no_longer_worth_listing() {
    let driver = FakeDriver::new();
    let game = Script { searches: searches(vec![Some(vec![]); 4]), ..script(vec![2, 3]) }.build();
    let skip_a = Recheck(|entry: &PlanEntry, _: &_| decision((entry.unique_id != "a").then_some(900), ""));
    let report = run_with(&plain(&driver, &game), &[inv("a", 0), inv("b", 1)], &skip_a);
    assert_eq!(uid_statuses(&report), [("a", "skipped"), ("b", "listed")]);
    assert_eq!(report.results[0].message, DEFAULT_SKIP_NOTE);
    assert!(driver.actions().contains(&Action::Type("900".into())));
    assert_eq!(driver.count(&Action::Click(point("create_listing_button"))), 1);
}

#[test]
fn recheck_skip_reason_is_shown() {
    let game = Script { searches: searches(vec![Some(vec![]), Some(vec![])]), ..script(vec![2]) }.build();
    let dropped = Recheck(|_: &PlanEntry, _: &_| decision(None, "market dropped to 500g"));
    let report = run_with(&plain(&FakeDriver::new(), &game), &[inv("a", 0)], &dropped);
    assert_eq!((report.results[0].status.as_str(), report.results[0].message.as_str()), ("skipped", "market dropped to 500g"));
}

#[test]
fn stacks_fill_the_quantity_box_before_the_price() {
    let driver = FakeDriver::new();
    let stack = PlanEntry { quantity: 3, ..entry("a", "2", 0, 270) };
    run(&plain(&driver, &script(used(2)).build()), &[stack]);
    let typed: Vec<Action> = driver.actions().into_iter().filter(|a| matches!(a, Action::Type(_))).collect();
    assert_eq!(typed, [Action::Type("3".into()), Action::Type("270".into())]);
    let clicks = driver.clicks();
    let quantity = clicks.iter().position(|c| *c == point("quantity_field")).unwrap();
    assert!(quantity < clicks.iter().position(|c| *c == point("price_field")).unwrap());
}
