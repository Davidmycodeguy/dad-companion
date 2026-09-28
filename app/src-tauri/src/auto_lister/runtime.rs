//! The lister's run machinery: the job that runs one lister operation at a time with the live
//! runners, the settings each run reads, and the market history a run's searches and crawls record.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use lister::job::{HoverFactory, ListerJob, MerchantRunnerFactory, RunnerFactory};
use lister::runner::{self, LiveConfig};

use crate::hover::facts;
use crate::input_lock::InputGuard;
use crate::state::AppState;

/// Where the Marketplace calibration offsets are saved, per resolution (DnDTools' key).
pub const CALIBRATION_KEY: &str = "marketplaceCalibrationOverride";
/// Seconds between steps of a run (DnDTools' `sortSpeed`).
const STEP_DELAY_KEY: &str = "sortSpeed";
/// How often the lock's watcher checks whether the run has finished.
const RUN_WATCH_INTERVAL: Duration = Duration::from_millis(200);
/// What the mouse lock calls the lister in "… is running." messages.
pub const LISTER_NAME: &str = "The auto lister";

/// The auto lister's job and what the page needs about the current run.
pub struct ListerRuntime {
    pub job: Arc<ListerJob>,
    /// Stash tab ids behind the Marketplace tab icons, for the character whose items are listed.
    tab_mapping: Arc<Mutex<Vec<i32>>>,
    /// How many items (or pages) the current run started with, for its progress bar.
    total: AtomicUsize,
}

impl ListerRuntime {
    pub fn new(app: &AppHandle) -> Self {
        let tab_mapping = Arc::new(Mutex::new(Vec::new()));
        let config = {
            let (app, tabs) = (app.clone(), Arc::clone(&tab_mapping));
            Arc::new(move || live_config(&app, &tabs))
        };
        let runner_factory: Arc<RunnerFactory> = {
            let (app, config) = (app.clone(), Arc::clone(&config));
            Arc::new(move |cancel| runner::live_runner(cancel, Arc::clone(&app.state::<AppState>().marketplace), config()))
        };
        let merchant_factory: Arc<MerchantRunnerFactory> = {
            let (app, config) = (app.clone(), Arc::clone(&config));
            Arc::new(move |cancel| runner::live_merchant_runner(cancel, Arc::clone(&app.state::<AppState>().merchant), config()))
        };
        let hover_factory: Arc<HoverFactory> = Arc::new(move |cancel| runner::live_hover_test(cancel, config()));
        ListerRuntime {
            job: Arc::new(ListerJob::new(runner_factory, hover_factory, Some(merchant_factory))),
            tab_mapping,
            total: AtomicUsize::new(0),
        }
    }

    /// Before a run: the tab order of the character whose items it lists, and its size.
    pub fn prepare(&self, tab_mapping: Vec<i32>, total: usize) {
        if let Ok(mut tabs) = self.tab_mapping.lock() {
            *tabs = tab_mapping;
        }
        self.total.store(total, Ordering::SeqCst);
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }

    /// Holds the mouse lock for the run just started: Ctrl+F12 and the lock's stop cancel it, and the
    /// lock is released once the run finishes.
    pub fn hold_until_done(&self, guard: InputGuard) {
        let job = Arc::clone(&self.job);
        guard.on_stop({
            let job = Arc::clone(&job);
            move || {
                job.cancel();
            }
        });
        let spawned = std::thread::Builder::new().name("lister-input-lock".into()).spawn(move || {
            while job.is_running() {
                std::thread::sleep(RUN_WATCH_INTERVAL);
            }
            drop(guard);
        });
        if let Err(err) = spawned {
            log::error!("could not watch the lister run: {err}");
        }
    }
}

/// The settings one live run reads, fresh for each run (calibration and tab order can change).
fn live_config(app: &AppHandle, tabs: &Mutex<Vec<i32>>) -> LiveConfig {
    let state = app.state::<AppState>();
    let (calibration, step_delay) = state
        .settings
        .lock()
        .map(|s| (s.get::<Value>(CALIBRATION_KEY).unwrap_or(Value::Null), s.get::<f64>(STEP_DELAY_KEY)))
        .unwrap_or((Value::Null, None));
    let mut config = LiveConfig::new(calibration, tabs.lock().map(|t| t.clone()).unwrap_or_default());
    if let Some(step_delay) = step_delay {
        config.step_delay_s = step_delay;
    }
    let scan_app = app.clone();
    config.scan_observer = Some(Arc::new(move |item_id: &str, started: f64, rows: &[market::MarketRow], complete: bool| {
        let state = scan_app.state::<AppState>();
        let Some(market) = state.market.as_ref() else { return };
        let max_price = rows.iter().map(|row| row.price).max().unwrap_or(0);
        let now = facts::now_s();
        if let Err(err) = market.note_scan(item_id, wall_time(&state, started, now), max_price, complete, now) {
            log::warn!("could not record the market scan of {item_id}: {err}");
        }
    }));
    let pass_app = app.clone();
    config.pass_observer = Some(Arc::new(move |rarity: i32, started: f64| {
        let state = pass_app.state::<AppState>();
        let market = state.market.as_ref()?;
        let now = facts::now_s();
        market
            .note_crawl_pass(i64::from(rarity), wall_time(&state, started, now), now)
            .inspect_err(|err| log::warn!("could not record the crawl of rarity {rarity}: {err}"))
            .ok()
            .map(|gone| gone as u64)
    }));
    config
}

/// A run's `started` (the marketplace state's own clock) as Unix seconds.
fn wall_time(state: &AppState, started: f64, now: f64) -> f64 {
    now - (state.marketplace.now() - started)
}
