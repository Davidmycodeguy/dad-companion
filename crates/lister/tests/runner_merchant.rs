//! Port of DnDTools' `tests/test_merchant_runner.py`: selling to The Collector.

#[path = "runner_fakes.rs"]
mod fakes;

use std::sync::Arc;

use fakes::*;
use input::marketplace::tab_icon_index;
use lister::job::{CancelToken, RunReport};
use lister::merchant_seller::{SELL_BOX_COLUMNS, SELL_BOX_ROWS};
use lister::plan::PlanEntry;
use lister::runner::merchant::{DRY_RUN_FIRST_BOX, MERCHANT_CARD_INDEX, NOTHING_SELLABLE, NOT_AT_MERCHANT, NO_DEAL_REPLY};
use lister::runner::{SafetyCheck, CURSOR_DEVIATION_PX, MOUSE_MOVED, OFF_LIMITS_REASON};

/// Sells `entries` to a fake Collector that takes everything dragged in.
fn sell_all(entries: &[PlanEntry], dry_run: bool) -> (RunReport, Arc<fakes::FakeDriver>, Arc<FakeMerchant>) {
    let (driver, game) = (FakeDriver::new(), merchant(entries).build());
    let report = sell(entries, &game, &driver, dry_run, &CancelToken::new(), None);
    (report, driver, game)
}

fn sell_on(entries: &[PlanEntry], game: Merchant) -> (RunReport, Arc<fakes::FakeDriver>, Arc<FakeMerchant>) {
    let (driver, game) = (FakeDriver::new(), game.build());
    let report = sell(entries, &game, &driver, false, &CancelToken::new(), None);
    (report, driver, game)
}

fn sell_with_safety(entries: &[PlanEntry], dry_run: bool, safety: Arc<dyn SafetyCheck>) -> (RunReport, Arc<fakes::FakeDriver>, Arc<FakeMerchant>) {
    let (driver, game) = (FakeDriver::new(), merchant(entries).build());
    let report = sell(entries, &game, &driver, dry_run, &CancelToken::new(), Some(safety));
    (report, driver, game)
}

/// Sells with the run cancelled right after the first drag.
fn sell_cancelled_after_first_drag(entries: &[PlanEntry], dry_run: bool) -> (RunReport, Arc<fakes::FakeDriver>, Arc<FakeMerchant>) {
    let (driver, game, cancel) = (FakeDriver::new(), merchant(entries).build(), CancelToken::new());
    let on_drag = cancel.clone();
    driver.on_action(move |action, _| {
        if matches!(action, Action::Drag(..)) {
            on_drag.cancel();
        }
    });
    let report = sell(entries, &game, &driver, dry_run, &cancel, None);
    (report, driver, game)
}

/// Sells while the player takes the mouse after each drag.
fn sell_with_mouse_taken(entries: &[PlanEntry]) -> (RunReport, Arc<fakes::FakeDriver>, Arc<FakeMerchant>) {
    let (driver, game) = (FakeDriver::new(), merchant(entries).build());
    driver.on_action(|action, driver| {
        if matches!(action, Action::Drag(..)) {
            let (x, y) = driver.pos();
            driver.set_pos((x + CURSOR_DEVIATION_PX as i32 + 50, y));
        }
    });
    let report = sell(entries, &game, &driver, false, &CancelToken::new(), None);
    (report, driver, game)
}

fn drags(driver: &fakes::FakeDriver) -> usize {
    driver.actions().iter().filter(|a| matches!(a, Action::Drag(..))).count()
}

fn make_deal_clicks(driver: &fakes::FakeDriver) -> usize {
    driver.count(&Action::Click(point("merchant_make_deal")))
}

fn tab(stash: &str) -> Action {
    Action::Click(layout().tab_icon(tab_icon_index(stash, &MERCHANT_MAPPING).unwrap() as i32))
}

#[test]
fn sell_opens_the_collector_stages_each_tab_and_makes_one_deal() {
    let entries = [sale_entry("powder", "4", 34, (1, 1), 250, 1), sale_entry("goblet", "20", 154, (1, 2), 100, 1)];
    let (report, driver, _) = sell_all(&entries, false);
    assert_eq!(
        driver.actions(),
        [
            Action::Click(point("merchants_tab")),
            Action::Click(layout().merchant_card(MERCHANT_CARD_INDEX)),
            Action::Click(point("merchant_sell_tab")),
            // Sell, never Buyback, before Make Deal.
            Action::Click(point("merchant_sell_mode")),
            tab("4"),
            Action::Drag(layout().item_centre("4", 34, 1, 1), layout().sell_box_centre(0, 0, 1, 1)),
            tab("20"),
            Action::Drag(layout().item_centre("20", 154, 1, 2), layout().sell_box_centre(1, 0, 1, 2)),
            Action::Click(point("merchant_make_deal")),
            Action::Escape,
        ]
    );
    assert_eq!(report.stopped_reason, None);
    assert_eq!(uid_statuses(&report), [("powder", "sold"), ("goblet", "sold")]);
    let messages: Vec<&str> = report.results.iter().map(|r| r.message.as_str()).collect();
    assert_eq!(messages, ["250g", "100g"]);
}

#[test]
fn stack_value_is_price_times_count() {
    let (report, _, _) = sell_all(&[sale_entry("eyes", "4", 0, (1, 1), 25, 2)], false);
    assert_eq!(report.results[0].message, "50g");
}

#[test]
fn wrong_or_missing_merchant_stops_before_any_item_is_touched() {
    let entries = [item("a", 0)];
    for opens in [None, Some("Weaponsmith".to_string())] {
        let (report, driver, game) = sell_on(&entries, Merchant { opens, ..merchant(&entries) });
        assert_eq!(report.stopped_reason.as_deref(), Some(NOT_AT_MERCHANT));
        assert!(!driver.actions().iter().any(|a| matches!(a, Action::Drag(..) | Action::Escape)));
        assert!(!driver.clicked(point("merchant_sell_tab")));
        assert!(game.deals().is_empty());
    }
}

#[test]
fn items_the_merchant_refuses_are_reported_not_taken() {
    let entries = [item("a", 0), item("b", 1)];
    let (report, _, _) = sell_on(&entries, Merchant { refuses: ["b".to_string()].into(), ..merchant(&entries) });
    assert_eq!(report.stopped_reason, None);
    assert_eq!(uid_statuses(&report), [("a", "sold"), ("b", "not_taken")]);
    assert!(report.results[1].message.contains("still in your stash"));
}

#[test]
fn selling_an_item_that_was_not_picked_stops_and_says_buy_it_back() {
    let entries = [item("a", 0), item("b", 200)];
    let (report, driver, _) = sell_on(&entries, Merchant { extra_sold: vec!["999".into()], ..merchant(&entries) });
    assert_eq!(uid_statuses(&report), [("a", "sold"), ("b", "sold")]);
    let reason = report.stopped_reason.unwrap();
    assert!(reason.contains("999") && reason.contains("Buyback"));
    // The merchant stays open so the item can be bought back.
    assert_ne!(driver.actions().last(), Some(&Action::Escape));
}

#[test]
fn no_reply_to_make_deal_stops() {
    let entries = [item("a", 0)];
    let (report, _, _) = sell_on(&entries, Merchant { answers: false, ..merchant(&entries) });
    assert_eq!(report.stopped_reason.as_deref(), Some(NO_DEAL_REPLY));
    assert!(report.results.is_empty());
}

#[test]
fn refused_deal_stops_and_sells_nothing() {
    let entries = [item("a", 0)];
    let (report, _, _) = sell_on(&entries, Merchant { result: 7, ..merchant(&entries) });
    assert!(report.stopped_reason.unwrap().contains('7'));
    assert!(report.results.is_empty());
}

#[test]
fn dry_run_stages_the_first_batch_then_escapes_without_a_deal() {
    let (report, driver, game) = sell_all(&[sale_entry("a", "4", 0, (1, 1), 5, 1)], true);
    assert!(!driver.clicked(point("merchant_make_deal")));
    assert_eq!(driver.actions().last(), Some(&Action::Escape));
    assert!(game.deals().is_empty());
    assert_eq!(uid_statuses(&report), [("a", "dry_run")]);
    assert!(report.results[0].message.contains("5g"));
}

#[test]
fn a_full_sell_box_is_sold_in_batches() {
    let count = (SELL_BOX_COLUMNS * SELL_BOX_ROWS + 2) as usize;
    let entries: Vec<PlanEntry> = (0..count).map(|i| item(&i.to_string(), i as i64)).collect();
    let (report, driver, game) = sell_all(&entries, false);
    let sizes: Vec<usize> = game.deals().iter().map(Vec::len).collect();
    assert_eq!(sizes, [count - 2, 2]);
    assert!(report.results.iter().all(|r| r.status == "sold"));
    assert_eq!(make_deal_clicks(&driver), 2);
}

#[test]
fn cancel_between_drags_stops_without_a_deal() {
    let (report, driver, game) = sell_cancelled_after_first_drag(&[item("a", 0), item("b", 1)], false);
    assert!(game.deals().is_empty());
    assert!(report.stopped_reason.unwrap().contains("Escape"));
    assert_eq!(drags(&driver), 1);
}

#[test]
fn safety_stop_mid_batch_never_clicks_make_deal() {
    let (report, _, game) = sell_with_safety(&[item("a", 0), item("b", 1)], false, FakeSafety::failing_after(2));
    assert!(game.deals().is_empty());
    assert!(report.stopped_reason.unwrap().contains("the game lost focus"));
}

#[test]
fn mouse_taken_during_staging_stops() {
    let (report, _, game) = sell_with_mouse_taken(&[item("a", 0), item("b", 1)]);
    assert!(game.deals().is_empty());
    assert!(report.stopped_reason.unwrap().starts_with(MOUSE_MOVED));
}

#[test]
fn items_in_unmapped_tabs_or_too_big_are_reported_and_the_rest_sold() {
    let huge = sale_entry("huge", "4", 0, (1, SELL_BOX_ROWS + 1), 10, 1);
    let entries = [sale_entry("eq", "3", 0, (1, 1), 10, 1), huge, item("a", 0)];
    let (report, _, game) = sell_all(&entries, false);
    assert_eq!(uid_statuses(&report), [("eq", "failed"), ("huge", "failed"), ("a", "sold")]);
    assert_eq!(game.deals(), [vec!["a".to_string()]]);
}

#[test]
fn seasonal_stash_items_are_refused_and_never_touched() {
    // Stash 30 has a tab icon (it is in the mapping), but the lister must never open it.
    let locked = sale_entry("s", "30", 0, (1, 1), 10, 1);
    // A different slot, so a drag from the locked item's spot would show.
    let (report, driver, game) = sell_all(&[locked, item("a", 5)], false);
    assert_eq!(uid_statuses(&report), [("s", "failed"), ("a", "sold")]);
    assert_eq!(report.results[0].message, OFF_LIMITS_REASON);
    assert!(!driver.actions().contains(&tab("30")));
    let from_locked = layout().item_centre("30", 0, 1, 1);
    assert!(!driver.actions().iter().any(|a| matches!(a, Action::Drag(from, _) if *from == from_locked)));
    assert_eq!(game.deals(), [vec!["a".to_string()]]);
}

#[test]
fn nothing_to_sell_does_not_touch_the_game() {
    let (report, driver, _) = sell_all(&[], false);
    assert!(driver.actions().is_empty());
    assert_eq!(report.stopped_reason.as_deref(), Some("Nothing to sell."));
}

#[test]
fn a_success_reply_that_sold_none_of_the_items_stops() {
    let entries = [item("a", 0), item("b", 1)];
    let (report, _, _) = sell_on(&entries, Merchant { sells_nothing: true, ..merchant(&entries) });
    assert!(report.results.is_empty());
    let reason = report.stopped_reason.unwrap();
    assert!(reason.contains("sold none") && reason.contains("Escape"));
}

#[test]
fn cancel_after_the_last_drag_never_clicks_make_deal() {
    let (report, driver, game) = sell_cancelled_after_first_drag(&[item("a", 0)], false);
    assert_eq!(make_deal_clicks(&driver), 0);
    assert!(game.deals().is_empty());
    let reason = report.stopped_reason.unwrap();
    assert!(reason.starts_with("Cancelled") && reason.contains("Escape"));
}

#[test]
fn focus_lost_after_the_last_drag_never_clicks_make_deal() {
    // Start and the drag pass; the check before the deal fails.
    let (report, driver, game) = sell_with_safety(&[item("a", 0)], false, FakeSafety::failing_after(2));
    assert_eq!(make_deal_clicks(&driver), 0);
    assert!(game.deals().is_empty());
    let reason = report.stopped_reason.unwrap();
    assert!(reason.contains("the game lost focus") && reason.contains("Escape"));
}

#[test]
fn mouse_taken_after_the_last_drag_never_clicks_make_deal() {
    let (report, driver, game) = sell_with_mouse_taken(&[item("a", 0)]);
    assert_eq!(make_deal_clicks(&driver), 0);
    assert!(game.deals().is_empty());
    let reason = report.stopped_reason.unwrap();
    assert!(reason.starts_with(MOUSE_MOVED) && reason.contains("Escape"));
}

#[test]
fn dry_run_never_sends_escape_after_focus_is_lost() {
    let (report, driver, _) = sell_with_safety(&[item("a", 0)], true, FakeSafety::failing_after(2));
    assert!(!driver.actions().contains(&Action::Escape));
    let reason = report.stopped_reason.unwrap();
    assert!(reason.contains("the game lost focus") && reason.contains("Escape"));
}

#[test]
fn cancelled_dry_run_puts_the_items_back_and_says_so() {
    let (report, driver, _) = sell_cancelled_after_first_drag(&[item("a", 0)], true);
    assert_eq!(driver.actions().last(), Some(&Action::Escape));
    assert!(report.stopped_reason.unwrap().starts_with("Cancelled"));
}

#[test]
fn dry_run_reports_items_beyond_the_first_sell_box() {
    let count = (SELL_BOX_COLUMNS * SELL_BOX_ROWS + 1) as usize;
    let entries: Vec<PlanEntry> = (0..count).map(|i| item(&i.to_string(), i as i64)).collect();
    let (report, _, _) = sell_all(&entries, true);
    assert_eq!(report.results.len(), count);
    let last = report.results.last().unwrap();
    assert_eq!((last.status.as_str(), last.message.as_str()), ("skipped", DRY_RUN_FIRST_BOX));
}

#[test]
fn nothing_sellable_is_a_stop_not_a_success() {
    let (report, driver, _) = sell_all(&[sale_entry("eq", "3", 0, (1, 1), 10, 1)], false);
    assert_eq!(report.stopped_reason.as_deref(), Some(NOTHING_SELLABLE));
    assert!(driver.actions().is_empty());
}
