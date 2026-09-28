//! Port of DnDTools' `tests/test_monitored_runner.py`, plus the `ListerJob` behaviors exercised
//! (through the out-of-scope Flask layer) in `tests/test_market_lister_api.py`'s fixtures: how a
//! run reaches `Done`, busy/backpressure, the price/crawl/collect/merchant-sell modes, the
//! `last_finished_at` / `last_list_finished_at` distinction, and cancellation.
//!
//! `test_monitored_runner.py`'s "plain attributes pass through unwrapped" case has no Rust
//! equivalent (see the doc comment on [`lister::job::MonitoredRunner`]) and is not ported.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use lister::job::{
    CancelToken, ItemResult, ListerJob, MerchantRunner, Mode, MonitoredRunner, Reprice, RunReport, Runner, RunState, Safety, NO_MERCHANT_RUNNER,
};
use lister::plan::{MarketBucket, PlanEntry};
use lister::FakeClock;
use market::MarketRow;

fn entry(uid: &str) -> PlanEntry {
    PlanEntry::new(uid, format!("Item {uid}"), 5, "2", 0, 1, 1, 900, 45, 10)
}

/// Bounded polling for a background job to finish: `ListerJob` (like the Python it ports) exposes
/// no completion signal beyond `status()`/`is_running()`, so this mirrors DnDTools' own
/// `test_market_lister_api.py::_wait_done` test helper rather than inventing new production API
/// just for tests.
fn wait_done(job: &ListerJob) {
    for _ in 0..500 {
        if !job.is_running() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("job did not finish in time");
}

#[derive(Default)]
struct RecordingMonitor {
    events: Mutex<Vec<&'static str>>,
}

impl Safety for RecordingMonitor {
    fn start(&self) {
        self.events.lock().unwrap().push("start");
    }
    fn stop(&self) {
        self.events.lock().unwrap().push("stop");
    }
}

/// A [`Runner`] whose `crawl_market` reports back double the page count (so a test can tell the
/// call actually reached it through [`MonitoredRunner`]) and whose `collect_payouts` panics (so a
/// test can check the monitor still stops).
struct DoublingRunner;

impl Runner for DoublingRunner {
    fn run(&self, _entries: &[PlanEntry], _dry_run: bool, _on_progress: &mut dyn FnMut(ItemResult), _reprice: Option<&dyn Reprice>) -> RunReport {
        RunReport { results: vec![], stopped_reason: None }
    }
    fn price_all(&self, _entries: &[PlanEntry], _on_progress: &mut dyn FnMut(ItemResult)) -> (HashMap<String, MarketBucket>, RunReport) {
        (HashMap::new(), RunReport { results: vec![], stopped_reason: None })
    }
    fn crawl_market(&self, pages: u32, _on_progress: &mut dyn FnMut(ItemResult), _is_old_page: Option<&lister::job::OldPageCheck>) -> RunReport {
        RunReport { results: vec![], stopped_reason: Some((pages * 2).to_string()) }
    }
    fn collect_payouts(&self, _on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        panic!("game closed")
    }
}

#[test]
fn actions_run_inside_the_safety_monitor() {
    let monitor = Arc::new(RecordingMonitor::default());
    let wrapped = MonitoredRunner::new(DoublingRunner, Arc::clone(&monitor) as Arc<dyn Safety>);
    let mut on_progress = |_r: ItemResult| {};
    let report = wrapped.crawl_market(3, &mut on_progress, None);
    assert_eq!(report.stopped_reason, Some("6".to_string()));
    assert_eq!(*monitor.events.lock().unwrap(), vec!["start", "stop"]);
}

#[test]
fn monitor_stops_even_when_an_action_fails() {
    let monitor = Arc::new(RecordingMonitor::default());
    let wrapped = MonitoredRunner::new(DoublingRunner, Arc::clone(&monitor) as Arc<dyn Safety>);
    let mut on_progress = |_r: ItemResult| {};
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| wrapped.collect_payouts(&mut on_progress)));
    assert!(outcome.is_err());
    assert_eq!(*monitor.events.lock().unwrap(), vec!["start", "stop"]);
}

// --- ListerJob ------------------------------------------------------------------------------

/// Mirrors DnDTools' `test_market_lister_api.py::FakeRunner`.
struct FakeRunner;

impl Runner for FakeRunner {
    fn run(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult), _reprice: Option<&dyn Reprice>) -> RunReport {
        let status = if dry_run { "dry_run" } else { "listed" };
        let results: Vec<ItemResult> = entries.iter().map(|e| ItemResult::new(&e.unique_id, &e.name, status)).collect();
        for r in results.clone() {
            on_progress(r);
        }
        RunReport { results, stopped_reason: None }
    }

    fn price_all(&self, entries: &[PlanEntry], on_progress: &mut dyn FnMut(ItemResult)) -> (HashMap<String, MarketBucket>, RunReport) {
        let rows = entries
            .iter()
            .map(|e| {
                let all = [300, 320, 340]
                    .iter()
                    .map(|&price| MarketRow { item_id: e.item_id.clone(), price, base: vec![], rolls: vec![], listing_id: String::new(), count: 1 })
                    .collect();
                (e.unique_id.clone(), MarketBucket { same: vec![], all, ..Default::default() })
            })
            .collect();
        let results: Vec<ItemResult> = entries.iter().map(|e| ItemResult::new(&e.unique_id, &e.name, "priced")).collect();
        for r in results.clone() {
            on_progress(r);
        }
        (rows, RunReport { results, stopped_reason: None })
    }

    fn crawl_market(&self, pages: u32, on_progress: &mut dyn FnMut(ItemResult), _is_old_page: Option<&lister::job::OldPageCheck>) -> RunReport {
        let result = ItemResult::new("crawl", "market", "crawled").with_message(format!("{pages} pages, {} listings", pages * 10));
        on_progress(result.clone());
        RunReport { results: vec![result], stopped_reason: None }
    }

    fn collect_payouts(&self, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        let result = ItemResult::new("1", "GreatHelm_3001", "collected").with_message("200g collected");
        on_progress(result.clone());
        RunReport { results: vec![result], stopped_reason: None }
    }
}

/// Mirrors DnDTools' `test_market_lister_api.py::FakeMerchantRunner`.
struct FakeMerchantRunner;

impl MerchantRunner for FakeMerchantRunner {
    fn sell(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        let status = if dry_run { "dry_run" } else { "sold" };
        let results: Vec<ItemResult> =
            entries.iter().map(|e| ItemResult::new(&e.unique_id, &e.name, status).with_message(format!("{}g", e.vendor_price))).collect();
        for r in results.clone() {
            on_progress(r);
        }
        RunReport { results, stopped_reason: None }
    }
}

/// A merchant runner that always panics, to exercise the "remind to empty the Sell box" hint.
struct BrokenMerchantRunner;

impl MerchantRunner for BrokenMerchantRunner {
    fn sell(&self, _entries: &[PlanEntry], _dry_run: bool, _on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        panic!("could not read the mouse position")
    }
}

/// Blocks `run` until released, so a test can observe `ListerJob` refusing a second `start()`
/// while one is already in flight — mirrors DnDTools' `test_market_lister_api.py::SlowRunner`.
#[derive(Default)]
struct Gate {
    released: Mutex<bool>,
    cond: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.cond.notify_all();
    }

    fn wait(&self) {
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.cond.wait_timeout(released, Duration::from_secs(2)).unwrap().0;
        }
    }
}

struct SlowRunner(Arc<Gate>);

impl Runner for SlowRunner {
    fn run(&self, _entries: &[PlanEntry], _dry_run: bool, _on_progress: &mut dyn FnMut(ItemResult), _reprice: Option<&dyn Reprice>) -> RunReport {
        self.0.wait();
        RunReport { results: vec![], stopped_reason: None }
    }
    fn price_all(&self, _entries: &[PlanEntry], _on_progress: &mut dyn FnMut(ItemResult)) -> (HashMap<String, MarketBucket>, RunReport) {
        (HashMap::new(), RunReport { results: vec![], stopped_reason: None })
    }
    fn crawl_market(&self, _pages: u32, _on_progress: &mut dyn FnMut(ItemResult), _is_old_page: Option<&lister::job::OldPageCheck>) -> RunReport {
        RunReport { results: vec![], stopped_reason: None }
    }
    fn collect_payouts(&self, _on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        RunReport { results: vec![], stopped_reason: None }
    }
}

fn no_op_hover(_token: CancelToken) -> Box<dyn FnOnce() + Send> {
    Box::new(|| {})
}

#[test]
fn start_runs_and_reports_status_for_list_and_dry_run() {
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), None);
    assert!(job.start(vec![entry("a")], true, None));
    wait_done(&job);
    let status = job.status();
    assert_eq!(status.state, RunState::Done);
    assert_eq!(status.mode, Some(Mode::DryRun));
    assert_eq!(status.results[0].status, "dry_run");
}

#[test]
fn start_returns_false_when_already_running() {
    let gate = Arc::new(Gate::default());
    let gate_for_runner = Arc::clone(&gate);
    let job =
        ListerJob::new(Arc::new(move |_token| Box::new(SlowRunner(Arc::clone(&gate_for_runner))) as Box<dyn Runner>), Arc::new(no_op_hover), None);
    assert!(job.start(vec![entry("a")], false, None));
    assert!(!job.start(vec![entry("a")], false, None));
    gate.release();
    wait_done(&job);
}

#[test]
fn price_sets_the_plan_on_the_status() {
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), None);
    let price_results = |entries: &[PlanEntry], rows: &HashMap<String, MarketBucket>| {
        lister::plan::apply_game_prices(entries, rows, &market::ListerRules::default(), None, 0.25, &HashSet::new(), None, None, None)
    };
    assert!(job.price(vec![entry("a")], price_results));
    wait_done(&job);
    let status = job.status();
    assert_eq!(status.state, RunState::Done);
    assert_eq!(status.mode, Some(Mode::Price));
    // The cheapest of FakeRunner's 300/320/340 quotes, undercut by the default 10%.
    assert_eq!(status.plan.unwrap().entries[0].price, 270);
}

#[test]
fn crawl_and_collect_run_and_record_results() {
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), None);
    assert!(job.crawl(5, None));
    wait_done(&job);
    let status = job.status();
    assert_eq!(status.mode, Some(Mode::Crawl));
    assert_eq!(status.results[0].message, "5 pages, 50 listings");

    assert!(job.collect());
    wait_done(&job);
    assert_eq!(job.status().results[0].status, "collected");
}

#[test]
fn sell_to_merchant_records_results_and_supports_dry_run() {
    let merchant_factory = Arc::new(|_token: CancelToken| Box::new(FakeMerchantRunner) as Box<dyn MerchantRunner>);
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), Some(merchant_factory));
    assert!(job.sell_to_merchant(vec![entry("a")], false));
    wait_done(&job);
    let status = job.status();
    assert_eq!(status.mode, Some(Mode::Merchant));
    assert_eq!(status.results[0].status, "sold");
    assert_eq!(job.sold_ids(), HashSet::from(["a".to_string()]));

    assert!(job.sell_to_merchant(vec![entry("b")], true));
    wait_done(&job);
    let status = job.status();
    assert_eq!(status.mode, Some(Mode::MerchantDryRun));
    assert_eq!(status.results[0].status, "dry_run");
}

#[test]
fn sell_to_merchant_without_a_merchant_factory_reports_unavailable() {
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), None);
    assert!(job.sell_to_merchant(vec![entry("a")], false));
    wait_done(&job);
    assert_eq!(job.status().stopped_reason.as_deref(), Some(NO_MERCHANT_RUNNER));
}

#[test]
fn merchant_job_errors_remind_to_empty_the_sell_box() {
    let merchant_factory = Arc::new(|_token: CancelToken| Box::new(BrokenMerchantRunner) as Box<dyn MerchantRunner>);
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), Some(merchant_factory));
    assert!(job.sell_to_merchant(vec![entry("a")], false));
    wait_done(&job);
    let reason = job.status().stopped_reason.unwrap();
    assert!(reason.contains("could not read the mouse position"));
    assert!(reason.contains("Escape"));
}

#[test]
fn last_finished_at_tracks_list_and_dry_run_differently() {
    let clock = Arc::new(FakeClock::new(100.0));
    let job = ListerJob::with_clock(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), Arc::new(no_op_hover), None, clock.clone());
    assert!(job.last_finished_at().is_none());
    assert!(job.last_list_finished_at().is_none());

    clock.set(101.0);
    job.start(vec![entry("a")], true, None);
    wait_done(&job);
    assert_eq!(job.last_finished_at(), Some(101.0));
    assert!(job.last_list_finished_at().is_none()); // a dry run never counts as "the last real list"

    clock.set(102.0);
    job.start(vec![entry("a")], false, None);
    wait_done(&job);
    assert_eq!(job.last_finished_at(), Some(102.0));
    assert_eq!(job.last_list_finished_at(), Some(102.0));
}

/// A [`Runner`] that waits (briefly polling, as a real cooperative runner would) for its own
/// [`CancelToken`] to be set, so the test can confirm `ListerJob::cancel` reaches it.
struct ObservingRunner(CancelToken, Arc<std::sync::atomic::AtomicBool>);

impl Runner for ObservingRunner {
    fn run(&self, _entries: &[PlanEntry], _dry_run: bool, _on_progress: &mut dyn FnMut(ItemResult), _reprice: Option<&dyn Reprice>) -> RunReport {
        for _ in 0..400 {
            if self.0.is_cancelled() {
                self.1.store(true, std::sync::atomic::Ordering::SeqCst);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        RunReport { results: vec![], stopped_reason: None }
    }
    fn price_all(&self, _entries: &[PlanEntry], _on_progress: &mut dyn FnMut(ItemResult)) -> (HashMap<String, MarketBucket>, RunReport) {
        (HashMap::new(), RunReport { results: vec![], stopped_reason: None })
    }
    fn crawl_market(&self, _pages: u32, _on_progress: &mut dyn FnMut(ItemResult), _is_old_page: Option<&lister::job::OldPageCheck>) -> RunReport {
        RunReport { results: vec![], stopped_reason: None }
    }
    fn collect_payouts(&self, _on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        RunReport { results: vec![], stopped_reason: None }
    }
}

#[test]
fn cancel_sets_the_token_the_runner_observes() {
    let saw_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_cancel_for_factory = Arc::clone(&saw_cancel);
    let runner_factory =
        Arc::new(move |token: CancelToken| Box::new(ObservingRunner(token, Arc::clone(&saw_cancel_for_factory))) as Box<dyn Runner>);
    let job = ListerJob::new(runner_factory, Arc::new(no_op_hover), None);
    assert!(job.start(vec![entry("a")], false, None));
    assert!(job.cancel());
    wait_done(&job);
    assert!(saw_cancel.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn hover_test_runs_and_reports_no_stopped_reason_when_not_cancelled() {
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran_for_factory = Arc::clone(&ran);
    let hover_factory = Arc::new(move |_token: CancelToken| {
        let ran = Arc::clone(&ran_for_factory);
        Box::new(move || ran.store(true, std::sync::atomic::Ordering::SeqCst)) as Box<dyn FnOnce() + Send>
    });
    let job = ListerJob::new(Arc::new(|_token| Box::new(FakeRunner) as Box<dyn Runner>), hover_factory, None);
    assert!(job.hover_test());
    wait_done(&job);
    assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
    let status = job.status();
    assert_eq!(status.mode, Some(Mode::Hover));
    assert!(status.stopped_reason.is_none());
}
