//! Runs one market lister operation at a time on a background thread.
//!
//! Port of DnDTools' `src/market_lister_job.py`. The runners that click through the game live in
//! [`crate::runner`] and implement [`Runner`], [`MerchantRunner`] and [`Safety`], matching the
//! Python's `InputDriver`, `Safety` and `MerchantInput` Protocols.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use market::MarketRow;

use crate::clock::{MonotonicClock, SharedClock};
use crate::plan::{MarketBucket, PlanEntry};

/// A cooperative cancellation flag: `cancel()` requests a running action stop; the action (the
/// `Runner`/`MerchantRunner` implementation) checks `is_cancelled()` between steps. Mirrors
/// Python's `threading.Event` used the same way here.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// One item's outcome during a run (listed, sold, priced, crawled, ...), reported through
/// `on_progress` as the run makes progress. A forward declaration of the shape DnDTools'
/// `marketplace_runner.ItemResult` dataclass has; the real type arrives with the runner phase.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ItemResult {
    pub unique_id: String,
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub message: String,
}

impl ItemResult {
    pub fn new(unique_id: impl Into<String>, name: impl Into<String>, status: impl Into<String>) -> Self {
        ItemResult { unique_id: unique_id.into(), name: name.into(), status: status.into(), message: String::new() }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
}

/// How a run ended: every item it processed, and why it stopped early (`None` if it ran to
/// completion). A forward declaration of `marketplace_runner.RunReport`; see [`ItemResult`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub results: Vec<ItemResult>,
    pub stopped_reason: Option<String>,
}

/// The last-second re-check `market_lister_api._repricer`/`_game_pricer` perform on an approved
/// price right before it is spent on a listing fee. That decision logic lives in the (out of scope
/// for this phase) API layer, not here; this is only the shape `Runner::run` passes it through as.
#[derive(Debug, Clone, PartialEq)]
pub struct RepriceDecision {
    /// The price to list at, or `None` to skip this entry (the market moved against it).
    pub price: Option<i64>,
    /// Why the price changed (or didn't), for the results table.
    pub note: String,
}

/// Re-prices one entry against fresh market rows just before it is listed.
pub trait Reprice: Send + Sync {
    fn reprice(&self, entry: &PlanEntry, market: &MarketBucket) -> RepriceDecision;
}

/// The sorter's safety monitor (paused-input detection etc.): wraps each runner action so it is
/// active only while that action runs. Mirrors DnDTools' `Safety` Protocol.
pub trait Safety: Send + Sync {
    fn start(&self);
    fn stop(&self);
}

/// What a crawl page looks like to `is_old_page`: just its rows (the paging and dedup logic lives in
/// [`crate::runner`]).
pub type CrawlPage = [MarketRow];
/// Recognises a page the crawl has already recorded, to stop an incremental crawl early.
pub type OldPageCheck = dyn Fn(&CrawlPage) -> bool + Send + Sync;

/// Drives the game through one Marketplace operation. Mirrors DnDTools' `InputDriver` Protocol;
/// [`crate::runner::live_runner`] implements it with the input crate.
pub trait Runner: Send {
    /// Lists (or, if `dry_run`, stages and puts back) each entry, calling `on_progress` as each
    /// one finishes. `reprice`, when given, re-checks the live market immediately before each
    /// listing and may lower the price or skip the entry.
    fn run(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult), reprice: Option<&dyn Reprice>) -> RunReport;

    /// Searches the market for each entry's exact item, for `apply_game_prices` to use.
    fn price_all(&self, entries: &[PlanEntry], on_progress: &mut dyn FnMut(ItemResult)) -> (std::collections::HashMap<String, MarketBucket>, RunReport);

    /// Pages through View Market recording listings to the local history. `is_old_page`, when
    /// given, stops an incremental crawl once it recognises a page it has already recorded.
    fn crawl_market(&self, pages: u32, on_progress: &mut dyn FnMut(ItemResult), is_old_page: Option<&OldPageCheck>) -> RunReport;

    /// Collects gold from sold/expired listings via Transfer All Items.
    fn collect_payouts(&self, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport;
}

/// Drives the game through a merchant sale. Mirrors DnDTools' `MerchantInput` Protocol;
/// [`crate::runner::live_merchant_runner`] implements it.
pub trait MerchantRunner: Send {
    fn sell(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport;
}

/// Builds a fresh runner for one job run, given the token that cancels it.
pub type RunnerFactory = dyn Fn(CancelToken) -> Box<dyn Runner> + Send + Sync;
pub type MerchantRunnerFactory = dyn Fn(CancelToken) -> Box<dyn MerchantRunner> + Send + Sync;
/// Builds a fresh hover-test action for one run: a one-shot calibration check with no return value.
pub type HoverFactory = dyn Fn(CancelToken) -> Box<dyn FnOnce() + Send> + Send + Sync;

/// Calls `monitor.stop()` when dropped, including when unwinding from a panic — the Rust
/// equivalent of Python's `try: ... finally: self._monitor.stop()`.
struct StopOnDrop<'a>(&'a dyn Safety);

impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// Runs each [`Runner`] action under the sorter's safety monitor: `monitor.start()` before, and
/// `monitor.stop()` after even if the action panics.
///
/// DnDTools' `MonitoredRunner` wraps *any* attribute of the underlying runner through
/// `__getattr__`, callable or not. Rust has no equivalent of that dynamic proxy, so this wraps
/// exactly the [`Runner`] methods `ListerJob` actually calls; a plain-attribute passthrough has no
/// meaningful Rust translation and is not attempted (see `tests/job.rs` for what was and wasn't
/// ported from `test_monitored_runner.py`).
pub struct MonitoredRunner<R> {
    runner: R,
    monitor: Arc<dyn Safety>,
}

impl<R> MonitoredRunner<R> {
    pub fn new(runner: R, monitor: Arc<dyn Safety>) -> Self {
        MonitoredRunner { runner, monitor }
    }

    fn guard(&self) -> StopOnDrop<'_> {
        self.monitor.start();
        StopOnDrop(&*self.monitor)
    }
}

impl<R: Runner> Runner for MonitoredRunner<R> {
    fn run(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult), reprice: Option<&dyn Reprice>) -> RunReport {
        let _guard = self.guard();
        self.runner.run(entries, dry_run, on_progress, reprice)
    }

    fn price_all(&self, entries: &[PlanEntry], on_progress: &mut dyn FnMut(ItemResult)) -> (std::collections::HashMap<String, MarketBucket>, RunReport) {
        let _guard = self.guard();
        self.runner.price_all(entries, on_progress)
    }

    fn crawl_market(&self, pages: u32, on_progress: &mut dyn FnMut(ItemResult), is_old_page: Option<&OldPageCheck>) -> RunReport {
        let _guard = self.guard();
        self.runner.crawl_market(pages, on_progress, is_old_page)
    }

    fn collect_payouts(&self, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        let _guard = self.guard();
        self.runner.collect_payouts(on_progress)
    }
}

impl<R: MerchantRunner> MerchantRunner for MonitoredRunner<R> {
    fn sell(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        let _guard = self.guard();
        self.runner.sell(entries, dry_run, on_progress)
    }
}

// --- ListerJob ------------------------------------------------------------------------------

/// What a job run is doing. `List`/`DryRun` are `LISTING_MODES` in the Python: finishing one of
/// them updates [`ListerJob::last_finished_at`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    DryRun,
    Price,
    Crawl,
    Collect,
    Merchant,
    MerchantDryRun,
    Hover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunState {
    #[default]
    Idle,
    Running,
    Done,
}

/// A snapshot of what the job is doing right now, or just finished doing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub state: RunState,
    pub mode: Option<Mode>,
    pub results: Vec<ItemResult>,
    pub stopped_reason: Option<String>,
    /// The plan a `price` run produced, once it is `Done`.
    pub plan: Option<crate::plan::Plan>,
}

/// No merchant runner was supplied to [`ListerJob::new`], so selling to merchants isn't wired up
/// in this build.
pub const NO_MERCHANT_RUNNER: &str = "Selling to merchants isn't available in this build.";
const SELL_BOX_HINT: &str = " If items are in the Sell box, press Escape in the game to put them back.";

#[derive(Default)]
struct JobState {
    thread: Option<JoinHandle<()>>,
    cancel: Option<CancelToken>,
    status: Status,
    last_finished_at: Option<f64>,
    last_list_finished_at: Option<f64>,
    /// Unique ids sold to merchants by this app session.
    sold_ids: HashSet<String>,
}

/// The state a background run thread must reach back into, held behind an `Arc` so the thread's
/// `'static` closure can share it with the `ListerJob` handle that spawned it (Python's closures
/// just capture `self`; Rust's `thread::spawn` needs an owned, `'static` handle instead).
struct Shared {
    clock: SharedClock,
    state: Mutex<JobState>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().expect("ListerJob mutex is never held across a panic")
    }

    /// Starts `mode` running `target` on a background thread, unless one is already running.
    ///
    /// Holds the lock across the whole check-then-spawn sequence (matching the Python's
    /// `with self._lock:` for the same method) so two concurrent callers can never both see "not
    /// running" and both spawn a thread. `target` must bring its own `Arc<Shared>` clone to report
    /// back with; `launch` only needs to store the `JoinHandle`.
    fn launch(&self, mode: Mode, target: impl FnOnce(CancelToken) + Send + 'static) -> bool {
        let mut state = self.lock();
        if state.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return false;
        }
        let token = CancelToken::new();
        state.cancel = Some(token.clone());
        state.status = Status { state: RunState::Running, mode: Some(mode), ..Status::default() };
        let handle = std::thread::Builder::new()
            .name("MarketLister".to_string())
            .spawn(move || target(token))
            // Invariant: this only fails when the OS refuses to create any new thread at all,
            // which leaves the app unable to do anything else useful either.
            .expect("failed to spawn the market lister thread");
        state.thread = Some(handle);
        true
    }

    fn finish(&self, stopped_reason: Option<String>) {
        let mut state = self.lock();
        state.status.state = RunState::Done;
        state.status.stopped_reason = stopped_reason;
        if matches!(state.status.mode, Some(Mode::List) | Some(Mode::DryRun)) {
            let now = self.clock.now();
            state.last_finished_at = Some(now);
            if state.status.mode == Some(Mode::List) {
                state.last_list_finished_at = Some(now);
            }
        }
    }

    fn record(&self, result: ItemResult) {
        let mut state = self.lock();
        if state.status.mode == Some(Mode::Merchant) && result.status == "sold" {
            state.sold_ids.insert(result.unique_id.clone());
        }
        state.status.results.push(result);
    }
}

/// Runs one market lister operation (list, price, crawl, collect, merchant sale, hover test) at a
/// time on a background thread, reporting progress and a final status other threads can poll.
pub struct ListerJob {
    runner_factory: Arc<RunnerFactory>,
    hover_factory: Arc<HoverFactory>,
    merchant_factory: Option<Arc<MerchantRunnerFactory>>,
    shared: Arc<Shared>,
}

impl ListerJob {
    pub fn new(runner_factory: Arc<RunnerFactory>, hover_factory: Arc<HoverFactory>, merchant_factory: Option<Arc<MerchantRunnerFactory>>) -> Self {
        Self::with_clock(runner_factory, hover_factory, merchant_factory, MonotonicClock::shared())
    }

    pub fn with_clock(
        runner_factory: Arc<RunnerFactory>,
        hover_factory: Arc<HoverFactory>,
        merchant_factory: Option<Arc<MerchantRunnerFactory>>,
        clock: SharedClock,
    ) -> Self {
        let shared = Arc::new(Shared { clock, state: Mutex::new(JobState::default()) });
        ListerJob { runner_factory, hover_factory, merchant_factory, shared }
    }

    /// When the last list / dry-run finished, or `None`.
    pub fn last_finished_at(&self) -> Option<f64> {
        self.shared.lock().last_finished_at
    }

    /// When the last real (non-dry) list run finished, or `None`.
    pub fn last_list_finished_at(&self) -> Option<f64> {
        self.shared.lock().last_list_finished_at
    }

    /// Items sold to a merchant; if the stash data still shows one, the data predates the sale.
    pub fn sold_ids(&self) -> HashSet<String> {
        self.shared.lock().sold_ids.clone()
    }

    pub fn is_running(&self) -> bool {
        let state = self.shared.lock();
        state.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    pub fn status(&self) -> Status {
        self.shared.lock().status.clone()
    }

    /// Requests the running action stop; `true` if one was running to cancel.
    pub fn cancel(&self) -> bool {
        let state = self.shared.lock();
        match (&state.cancel, &state.thread) {
            (Some(token), Some(t)) if !t.is_finished() => {
                token.cancel();
                true
            }
            _ => false,
        }
    }

    /// Lists (or dry-runs) `entries`. `reprice`, when given, re-checks the live market immediately
    /// before each listing.
    pub fn start(&self, entries: Vec<PlanEntry>, dry_run: bool, reprice: Option<Arc<dyn Reprice>>) -> bool {
        let runner_factory = Arc::clone(&self.runner_factory);
        let shared = Arc::clone(&self.shared);
        let mode = if dry_run { Mode::DryRun } else { Mode::List };
        self.shared.launch(mode, move |token| {
            let outcome = run_guarded(|| {
                let runner = runner_factory(token);
                let mut on_progress = |result: ItemResult| shared.record(result);
                runner.run(&entries, dry_run, &mut on_progress, reprice.as_deref()).stopped_reason
            });
            shared.finish(outcome);
        })
    }

    /// Prices `entries` from the in-game market; `price_results(entries, rows)` builds the plan
    /// that ends up in [`Status::plan`].
    pub fn price(&self, entries: Vec<PlanEntry>, price_results: impl FnOnce(&[PlanEntry], &std::collections::HashMap<String, MarketBucket>) -> crate::plan::Plan + Send + 'static) -> bool {
        let runner_factory = Arc::clone(&self.runner_factory);
        let shared = Arc::clone(&self.shared);
        self.shared.launch(Mode::Price, move |token| {
            let shared_for_plan = Arc::clone(&shared);
            let outcome = run_guarded(move || {
                let runner = runner_factory(token);
                let mut on_progress = |result: ItemResult| shared_for_plan.record(result);
                let (rows, report) = runner.price_all(&entries, &mut on_progress);
                shared_for_plan.lock().status.plan = Some(price_results(&entries, &rows));
                report.stopped_reason
            });
            shared.finish(outcome);
        })
    }

    /// Pages through View Market recording listings to the local history.
    pub fn crawl(&self, pages: u32, is_old_page: Option<Arc<OldPageCheck>>) -> bool {
        let runner_factory = Arc::clone(&self.runner_factory);
        let shared = Arc::clone(&self.shared);
        self.shared.launch(Mode::Crawl, move |token| {
            let outcome = run_guarded(|| {
                let runner = runner_factory(token);
                let mut on_progress = |result: ItemResult| shared.record(result);
                let is_old_page = is_old_page.as_deref();
                runner.crawl_market(pages, &mut on_progress, is_old_page).stopped_reason
            });
            shared.finish(outcome);
        })
    }

    /// Collects gold from sold/expired listings.
    pub fn collect(&self) -> bool {
        let runner_factory = Arc::clone(&self.runner_factory);
        let shared = Arc::clone(&self.shared);
        self.shared.launch(Mode::Collect, move |token| {
            let outcome = run_guarded(|| {
                let runner = runner_factory(token);
                let mut on_progress = |result: ItemResult| shared.record(result);
                runner.collect_payouts(&mut on_progress).stopped_reason
            });
            shared.finish(outcome);
        })
    }

    /// Sells `entries` to a merchant; a dry run stages them and puts them back.
    pub fn sell_to_merchant(&self, entries: Vec<PlanEntry>, dry_run: bool) -> bool {
        let merchant_factory = self.merchant_factory.clone();
        let shared = Arc::clone(&self.shared);
        let mode = if dry_run { Mode::MerchantDryRun } else { Mode::Merchant };
        self.shared.launch(mode, move |token| {
            let Some(merchant_factory) = merchant_factory else {
                shared.finish(Some(NO_MERCHANT_RUNNER.to_string()));
                return;
            };
            // Only an unexpected error can leave items in the Sell box: its message says what to do.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runner = merchant_factory(token);
                let mut on_progress = |result: ItemResult| shared.record(result);
                runner.sell(&entries, dry_run, &mut on_progress).stopped_reason
            }))
            .unwrap_or_else(|payload| Some(format!("Unexpected error: {}.{SELL_BOX_HINT}", panic_message(&*payload))));
            shared.finish(outcome);
        })
    }

    pub fn hover_test(&self) -> bool {
        let hover_factory = Arc::clone(&self.hover_factory);
        let shared = Arc::clone(&self.shared);
        self.shared.launch(Mode::Hover, move |token| {
            // Checked after the action runs, not before: the action itself is what a `cancel()`
            // call from another thread interrupts, so only then does the flag mean anything.
            let check_cancelled = token.clone();
            let action = hover_factory(token);
            let outcome = run_guarded(move || {
                action();
                check_cancelled.is_cancelled().then(|| "Cancelled".to_string())
            });
            shared.finish(outcome);
        })
    }
}

/// Runs `body`, catching a panic the way Python's `except Exception` catches a runner bug, so the
/// job never gets stuck reporting "running" forever. Returns `body`'s own stopped reason on
/// success, or `Some("Unexpected error: ...")` if it panicked.
///
/// `AssertUnwindSafe` is the standard escape hatch at exactly this kind of boundary: everything
/// `body` touches lives behind a `Mutex` (poison-safe by construction) or is about to be
/// overwritten by `Shared::finish` regardless of whether the panic left it mid-update.
fn run_guarded(body: impl FnOnce() -> Option<String>) -> Option<String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(stopped_reason) => stopped_reason,
        // `&*payload`, not `&payload`: `Box<dyn Any + Send>` also (trivially) implements `Any`
        // itself, so a bare `&payload` coerces to "the box is the Any", carrying the wrong
        // vtable for `downcast_ref` — derefing first gets the vtable of what's actually inside.
        Err(payload) => Some(format!("Unexpected error: {}", panic_message(&*payload))),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
