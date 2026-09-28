//! The runners the app builds for each job run (bring the game forward, lay out its screens, run
//! under a fresh safety monitor) and the hover test — ports of DnDTools' `app.py` factories — driven
//! by fakes: nothing here touches the real mouse, keyboard or game window.

#[path = "runner_fakes.rs"]
mod fakes;

use std::sync::Arc;
use std::time::{Duration, Instant};

use fakes::*;
use input::marketplace::build_layout;
use lister::job::{CancelToken, ListerJob, RunState};
use lister::marketplace_state::{ListingsSnapshot, RegisterOutcome};
use lister::runner::live::{sleep_unless_cancelled, step_pause};
use lister::runner::{hover_test_in, merchant_runner_in, runner_in, LiveConfig, MarketplaceGame};
use market::MarketRow;
use serde_json::{json, Value};

const NOT_FOUND: &str = "Dark and Darker window not found.";

fn config() -> LiveConfig {
    LiveConfig::new(Value::Null, MAPPING.to_vec())
}

/// A game whose answers to My Listings take `delay` (the game being slow to answer).
struct SlowGame<G> {
    inner: Arc<G>,
    delay: Duration,
}

impl<G: MarketplaceGame> MarketplaceGame for SlowGame<G> {
    fn now(&self) -> f64 {
        self.inner.now()
    }
    fn snapshot(&self) -> Option<ListingsSnapshot> {
        self.inner.snapshot()
    }
    fn wait_for_fresh_snapshot(&self, since: f64, timeout: f64) -> Option<ListingsSnapshot> {
        std::thread::sleep(self.delay);
        self.inner.wait_for_fresh_snapshot(since, timeout)
    }
    fn wait_for_item_list(&self, since: f64, timeout: f64) -> Option<Vec<MarketRow>> {
        self.inner.wait_for_item_list(since, timeout)
    }
    fn last_item_page(&self) -> Option<(i64, i64)> {
        self.inner.last_item_page()
    }
    fn begin_transfer(&self) {
        self.inner.begin_transfer();
    }
    fn wait_for_transfer(&self, timeout: f64) -> Option<i64> {
        self.inner.wait_for_transfer(timeout)
    }
    fn begin_register(&self) {
        self.inner.begin_register();
    }
    fn wait_for_register(&self, timeout: f64) -> RegisterOutcome {
        self.inner.wait_for_register(timeout)
    }
    fn wait_for_listing(&self, unique_id: &str, since: f64, timeout: f64) -> bool {
        self.inner.wait_for_listing(unique_id, since, timeout)
    }
}

/// A game with nothing to collect: a collect run only re-opens My Listings.
fn nothing_to_collect() -> Arc<PayoutGame> {
    PayoutGame::new(vec![payout_snapshot(vec![]), payout_snapshot(vec![])], vec![])
}

#[test]
fn a_game_that_cannot_be_brought_forward_stops_every_run_before_any_input() {
    let (window, driver, desktop) = (FakeWindow::failing(NOT_FOUND), FakeDriver::new(), FakeDesktop::new());
    let env = fake_environment(&window, &driver, &desktop);
    let expected = Some("Unexpected error: Dark and Darker window not found.");
    let runner = runner_in(env.clone(), CancelToken::new(), script(used(0)).build(), config());
    let entries = [entry("a", "2", 0, 900)];
    assert_eq!(runner.run(&entries, false, &mut |_| {}, None).stopped_reason.as_deref(), expected);
    let (rows, report) = runner.price_all(&entries, &mut |_| {});
    assert!(rows.is_empty());
    assert_eq!(report.stopped_reason.as_deref(), expected);
    assert_eq!(runner.crawl_market(5, &mut |_| {}, None).stopped_reason.as_deref(), expected);
    assert_eq!(runner.collect_payouts(&mut |_| {}).stopped_reason.as_deref(), expected);
    let seller = merchant_runner_in(env, CancelToken::new(), merchant(&[item("a", 0)]).build(), config());
    assert_eq!(seller.sell(&[item("a", 0)], false, &mut |_| {}).stopped_reason.as_deref(), expected);
    assert_eq!(window.calls(), 5);
    assert!(driver.actions().is_empty());
}

#[test]
fn the_layout_follows_the_game_window() {
    // A windowed game whose client area starts at (100, 50).
    let (window, driver, desktop) = (FakeWindow::at(100, 50, 1920, 1080), FakeDriver::new(), FakeDesktop::new());
    let runner = runner_in(fake_environment(&window, &driver, &desktop), CancelToken::new(), nothing_to_collect(), config());
    let report = runner.collect_payouts(&mut |_| {});
    assert_eq!(report.stopped_reason, None);
    let windowed = build_layout((1920, 1080), (100, 50), &Value::Null);
    assert_eq!(driver.clicks(), [windowed.point("view_market_tab"), windowed.point("my_listings_tab")]);
}

#[test]
fn the_calibration_saved_for_the_game_resolution_is_applied() {
    let calibration = json!({
        "2560x1440": {"points": {"view_market_tab": [50, 50]}},
        "1920x1080": {"points": {"view_market_tab": [3, -2]}},
    });
    let (window, driver, desktop) = (FakeWindow::at(0, 0, 1920, 1080), FakeDriver::new(), FakeDesktop::new());
    let config = LiveConfig { calibration, ..config() };
    let runner = runner_in(fake_environment(&window, &driver, &desktop), CancelToken::new(), nothing_to_collect(), config);
    runner.collect_payouts(&mut |_| {});
    let (x, y) = point("view_market_tab");
    assert_eq!(driver.clicks()[0], (x + 3, y - 2));
}

#[test]
fn each_action_runs_under_a_fresh_safety_monitor() {
    let (window, driver, desktop) = (FakeWindow::at(0, 0, 1920, 1080), FakeDriver::new(), FakeDesktop::new());
    let game = Arc::new(SlowGame { inner: nothing_to_collect(), delay: Duration::from_millis(450) });
    let runner = runner_in(fake_environment(&window, &driver, &desktop), CancelToken::new(), game, config());
    let report = runner.collect_payouts(&mut |_| {});
    assert_eq!(report.stopped_reason, None);
    // The runner's checkpoints read the cursor, and the focus watcher ran during the action...
    assert!(desktop.cursor_reads() > 0);
    let polls = desktop.focus_polls();
    assert!(polls > 0);
    // ...and stopped with it.
    std::thread::sleep(Duration::from_millis(450));
    assert_eq!(desktop.focus_polls(), polls);
}

#[test]
fn the_monitor_stops_a_run_once_the_game_loses_focus() {
    let (window, driver, desktop) = (FakeWindow::at(0, 0, 1920, 1080), FakeDriver::new(), FakeDesktop::new());
    desktop.set_focused(false);
    let payout = (1, 3, "GreatHelm_3001", 200);
    let inner = PayoutGame::new(vec![payout_snapshot(vec![payout]), payout_snapshot(vec![payout])], vec![1]);
    // My Listings answers well after the focus watcher trips (about 0.8 s in).
    let game = Arc::new(SlowGame { inner, delay: Duration::from_millis(1600) });
    let runner = runner_in(fake_environment(&window, &driver, &desktop), CancelToken::new(), game, config());
    let report = runner.collect_payouts(&mut |_| {});
    assert_eq!(report.stopped_reason.as_deref(), Some("Stopped for safety: the game lost focus"));
    assert!(!driver.clicked(point("transfer_all_button")));
}

#[test]
fn hover_test_rests_on_every_spot_without_clicking() {
    let (window, driver, desktop) = (FakeWindow::at(0, 0, 1920, 1080), FakeDriver::new(), FakeDesktop::new());
    hover_test_in(fake_environment(&window, &driver, &desktop), CancelToken::new(), config())();
    let expected: Vec<Action> = layout().hover_targets().into_iter().map(|(_, spot)| Action::Move(spot)).collect();
    assert_eq!(expected.len(), 11);
    assert_eq!(driver.actions(), expected);
}

#[test]
fn a_cancelled_hover_test_stops_moving() {
    let (window, driver, desktop) = (FakeWindow::at(0, 0, 1920, 1080), FakeDriver::new(), FakeDesktop::new());
    let cancel = CancelToken::new();
    let on_move = cancel.clone();
    driver.on_action(move |_, driver| {
        if driver.actions().len() == 3 {
            on_move.cancel();
        }
    });
    hover_test_in(fake_environment(&window, &driver, &desktop), cancel, config())();
    assert_eq!(driver.actions().len(), 3);
}

#[test]
fn a_hover_test_that_cannot_find_the_game_says_so_through_the_job() {
    let (window, driver, desktop) = (FakeWindow::failing(NOT_FOUND), FakeDriver::new(), FakeDesktop::new());
    let env = fake_environment(&window, &driver, &desktop);
    let runner_env = env.clone();
    let job = ListerJob::new(
        Arc::new(move |cancel| runner_in(runner_env.clone(), cancel, script(used(0)).build(), config())),
        Arc::new(move |cancel| hover_test_in(env.clone(), cancel, config())),
        None,
    );
    assert!(job.hover_test());
    let started = Instant::now();
    while job.is_running() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let status = job.status();
    assert_eq!(status.state, RunState::Done);
    assert_eq!(status.stopped_reason.as_deref(), Some("Unexpected error: Dark and Darker window not found."));
    assert!(driver.actions().is_empty());
}

#[test]
fn the_pause_after_a_step_follows_the_sort_speed_with_a_little_jitter() {
    let within = |pause: Duration, low: f64| (low..low + 0.07).contains(&pause.as_secs_f64());
    assert!(within(step_pause(0.0), 0.15), "never below the minimum");
    assert!(within(step_pause(0.5), 0.5));
    assert!(within(step_pause(-1.0), 0.2), "a broken setting means the default");
    assert!(within(step_pause(f64::NAN), 0.2));
    assert!(within(step_pause(1e12), 10.0), "bounded");
}

#[test]
fn a_cancelled_sleep_returns_at_once() {
    let cancel = CancelToken::new();
    cancel.cancel();
    let started = Instant::now();
    sleep_unless_cancelled(&cancel, Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// Read-only window lookups: sends no input, safe with the game open.
#[test]
fn reading_the_game_resolution_does_not_panic() {
    let _ = lister::runner::game_resolution();
}
