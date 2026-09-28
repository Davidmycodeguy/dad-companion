//! What the app calls: runners that click in the real game, built fresh for each job run.
//!
//! Port of DnDTools' `marketplace_input.py` (real input, the layout for the current resolution, the
//! pause between steps) and the runner / hover-test factories in `app.py`: each run first brings the
//! game forward, then runs its action under a fresh [`SafetyMonitor`] (started before the action,
//! stopped after, as [`MonitoredRunner`] does).
//!
//! # Wiring
//!
//! ```ignore
//! let runner_factory: Arc<RunnerFactory> = Arc::new(move |cancel| {
//!     runner::live_runner(cancel, Arc::clone(&marketplace), config_now())
//! });
//! ```
//!
//! Build the [`LiveConfig`] inside the factory, on every call: the calibration and the stash tab
//! order can change between runs.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use input::marketplace::{build_layout, MarketplaceLayout};
use input::WindowRect;
use serde_json::Value;

use super::marketplace::{PassObserver, ScanObserver};
use super::{
    hover, refused, unexpected, Desktop, GameInput, MarketplaceGame, MarketplaceRunner, MerchantGame,
    MerchantSaleRunner, SafetyMonitor,
};
use crate::job::{CancelToken, ItemResult, MerchantRunner, MonitoredRunner, OldPageCheck, Reprice, RunReport, Runner};
use crate::marketplace_state::MarketplaceState;
use crate::merchant_state::MerchantState;
use crate::plan::{MarketBucket, PlanEntry};

mod desktop;
mod driver;

pub use desktop::{sleep_unless_cancelled, step_pause, LiveDesktop, LiveGameWindow};
pub use driver::LiveInput;

/// DnDTools' default `sortSpeed` (seconds between steps).
pub const DEFAULT_STEP_DELAY_S: f64 = 0.2;
/// The shortest pause between steps, whatever `sortSpeed` says.
pub const MIN_STEP_DELAY_S: f64 = 0.15;
/// Up to this much is added to each pause at random.
pub const STEP_JITTER_S: f64 = 0.07;
/// Longest pause taken from `sortSpeed` (a sanity bound for a broken setting).
pub const MAX_STEP_DELAY_S: f64 = 10.0;

/// The settings one live run reads.
#[derive(Clone)]
pub struct LiveConfig {
    /// The `marketplaceCalibrationOverride` setting as saved: `{"1920x1080": {"points": {key: [dx,
    /// dy]}, "lengths": {key: d}}, ...}`. The entry for the game's resolution (its window's client
    /// size, see [`game_resolution`]) is applied; no entry, or `Value::Null`, means none.
    pub calibration: Value,
    /// The stash id behind each Marketplace tab icon after the inventory's, in icon order: the
    /// character's stash ids from 4 to 99, ascending (DnDTools' `auto_tab_order`).
    pub tab_mapping: Vec<i32>,
    /// The `sortSpeed` setting: seconds to pause after each step (at least [`MIN_STEP_DELAY_S`],
    /// plus up to [`STEP_JITTER_S`]); negative or not a number means [`DEFAULT_STEP_DELAY_S`].
    pub step_delay_s: f64,
    /// The game's process name, for the focus checks ([`input::GAME_PROCESS_EXE`]).
    pub game_exe: String,
    /// Told about each item's market search (to mark vanished listings sold in the history).
    pub scan_observer: Option<Arc<ScanObserver>>,
    /// Told when a crawl read a whole rarity; returns how many listings are gone.
    pub pass_observer: Option<Arc<PassObserver>>,
}

impl LiveConfig {
    /// `calibration` and `tab_mapping` as described on the fields; the defaults for the rest.
    pub fn new(calibration: Value, tab_mapping: Vec<i32>) -> Self {
        LiveConfig {
            calibration,
            tab_mapping,
            step_delay_s: DEFAULT_STEP_DELAY_S,
            game_exe: input::GAME_PROCESS_EXE.to_string(),
            scan_observer: None,
            pass_observer: None,
        }
    }
}

/// Brings the game forward before a run.
pub trait GameWindow: Send + Sync {
    /// Brings the game's window to the front (DnDTools' `_focus_game_window`) and returns its client
    /// area in screen pixels, or the message for the player when it can't. May wait, returning
    /// early once `cancel` is cancelled.
    fn bring_forward(&self, cancel: &CancelToken) -> Result<WindowRect, String>;
}

/// A sleep that returns early once the token is cancelled.
pub type SleepFn = dyn Fn(&CancelToken, Duration) + Send + Sync;

/// The machine a live run works on. [`Environment::live`] is the real one; tests pass fakes.
#[derive(Clone)]
pub struct Environment {
    pub window: Arc<dyn GameWindow>,
    pub input: Arc<dyn GameInput>,
    /// What the safety monitor watches.
    pub desktop: Arc<dyn Desktop>,
    /// How the pauses between steps and the hover test's rests wait.
    pub sleep: Arc<SleepFn>,
}

impl Environment {
    /// The real game window, mouse, keyboard and clock; `game_exe` is the game's process name.
    pub fn live(game_exe: &str) -> Self {
        Environment {
            window: Arc::new(LiveGameWindow::new(game_exe)),
            input: Arc::new(LiveInput),
            desktop: Arc::new(LiveDesktop::new(game_exe)),
            sleep: Arc::new(sleep_unless_cancelled),
        }
    }

    /// Brings the game forward and lays out its Marketplace screens for its current resolution.
    fn layout(&self, cancel: &CancelToken, calibration: &Value) -> Result<MarketplaceLayout, String> {
        let area = self.window.bring_forward(cancel).map_err(|message| unexpected(&message))?;
        Ok(layout_for(&area, calibration))
    }

    fn monitor(&self, cancel: &CancelToken) -> Arc<SafetyMonitor> {
        Arc::new(SafetyMonitor::new(cancel.clone(), Arc::clone(&self.desktop)))
    }

    /// The pause after each step (DnDTools' `make_pause`).
    fn pause(&self, cancel: &CancelToken, step_delay_s: f64) -> Box<dyn Fn() + Send> {
        let (sleep, cancel) = (Arc::clone(&self.sleep), cancel.clone());
        Box::new(move || sleep(&cancel, step_pause(step_delay_s)))
    }
}

/// The Marketplace layout for a game window with client area `area`: its size is the resolution,
/// its top-left the origin (windowed mode, or a monitor other than the primary one).
fn layout_for(area: &WindowRect, calibration: &Value) -> MarketplaceLayout {
    let resolution = (area.width.unsigned_abs(), area.height.unsigned_abs());
    let key = format!("{}x{}", resolution.0, resolution.1);
    build_layout(resolution, (area.left, area.top), calibration.get(&key).unwrap_or(&Value::Null))
}

/// The Marketplace runner for one job run (`RunnerFactory`): list, price from the game, crawl and
/// collect. `marketplace` must be the state the packet reader feeds (see the crate docs of
/// [`crate::runner`] for the messages it needs).
pub fn live_runner(cancel: CancelToken, marketplace: Arc<MarketplaceState>, config: LiveConfig) -> Box<dyn Runner> {
    runner_in(Environment::live(&config.game_exe), cancel, marketplace, config)
}

/// [`live_runner`] on a given [`Environment`].
pub fn runner_in(env: Environment, cancel: CancelToken, game: Arc<dyn MarketplaceGame>, config: LiveConfig) -> Box<dyn Runner> {
    Box::new(LiveMarketplace { env, cancel, game, config })
}

/// The merchant runner for one job run (`MerchantRunnerFactory`); `merchant` must be the state the
/// packet reader feeds. The observers of `config` are not used.
pub fn live_merchant_runner(cancel: CancelToken, merchant: Arc<MerchantState>, config: LiveConfig) -> Box<dyn MerchantRunner> {
    merchant_runner_in(Environment::live(&config.game_exe), cancel, merchant, config)
}

/// [`live_merchant_runner`] on a given [`Environment`].
pub fn merchant_runner_in(
    env: Environment,
    cancel: CancelToken,
    game: Arc<dyn MerchantGame>,
    config: LiveConfig,
) -> Box<dyn MerchantRunner> {
    Box::new(LiveMerchant { env, cancel, game, config })
}

/// The hover test for one job run (`HoverFactory`): brings the game forward, then rests the cursor
/// on each Marketplace spot for a second without clicking. Uses `config`'s calibration and
/// `game_exe` only.
///
/// When the game can't be brought forward, the action panics with the message (without running
/// the panic hook): the hover factory's action has no other way to report it, and the job turns the
/// panic into "Unexpected error: …", the text DnDTools showed for it.
pub fn live_hover_test(cancel: CancelToken, config: LiveConfig) -> Box<dyn FnOnce() + Send> {
    hover_test_in(Environment::live(&config.game_exe), cancel, config)
}

/// [`live_hover_test`] on a given [`Environment`].
pub fn hover_test_in(env: Environment, cancel: CancelToken, config: LiveConfig) -> Box<dyn FnOnce() + Send> {
    Box::new(move || {
        if let Err(message) = hover_test(&env, &cancel, &config.calibration) {
            std::panic::resume_unwind(Box::new(message));
        }
    })
}

fn hover_test(env: &Environment, cancel: &CancelToken, calibration: &Value) -> Result<(), String> {
    let area = env.window.bring_forward(cancel)?;
    if cancel.is_cancelled() {
        return Ok(());
    }
    let layout = layout_for(&area, calibration);
    let wait = |duration| (env.sleep)(cancel, duration);
    hover::hover_over(&*env.input, &layout.hover_targets(), cancel, &wait).map_err(|error| error.0)
}

/// Brings the game forward before each action, then runs it under a fresh safety monitor.
struct LiveMarketplace {
    env: Environment,
    cancel: CancelToken,
    game: Arc<dyn MarketplaceGame>,
    config: LiveConfig,
}

impl LiveMarketplace {
    fn prepare(&self) -> Result<MonitoredRunner<MarketplaceRunner>, String> {
        let layout = self.env.layout(&self.cancel, &self.config.calibration)?;
        let monitor = self.env.monitor(&self.cancel);
        let pause = self.env.pause(&self.cancel, self.config.step_delay_s);
        let tab_mapping = self.config.tab_mapping.clone();
        let runner = MarketplaceRunner::new(Arc::clone(&self.env.input), layout, Arc::clone(&self.game), tab_mapping, self.cancel.clone(), pause)
            .with_safety(monitor.clone())
            .with_scan_observer(self.config.scan_observer.clone())
            .with_pass_observer(self.config.pass_observer.clone());
        Ok(MonitoredRunner::new(runner, monitor))
    }
}

impl Runner for LiveMarketplace {
    fn run(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult), reprice: Option<&dyn Reprice>) -> RunReport {
        match self.prepare() {
            Ok(runner) => runner.run(entries, dry_run, on_progress, reprice),
            Err(reason) => refused(reason),
        }
    }

    fn price_all(&self, entries: &[PlanEntry], on_progress: &mut dyn FnMut(ItemResult)) -> (HashMap<String, MarketBucket>, RunReport) {
        match self.prepare() {
            Ok(runner) => runner.price_all(entries, on_progress),
            Err(reason) => (HashMap::new(), refused(reason)),
        }
    }

    fn crawl_market(&self, pages: u32, on_progress: &mut dyn FnMut(ItemResult), is_old_page: Option<&OldPageCheck>) -> RunReport {
        match self.prepare() {
            Ok(runner) => runner.crawl_market(pages, on_progress, is_old_page),
            Err(reason) => refused(reason),
        }
    }

    fn collect_payouts(&self, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        match self.prepare() {
            Ok(runner) => runner.collect_payouts(on_progress),
            Err(reason) => refused(reason),
        }
    }
}

/// [`LiveMarketplace`] for merchant sales.
struct LiveMerchant {
    env: Environment,
    cancel: CancelToken,
    game: Arc<dyn MerchantGame>,
    config: LiveConfig,
}

impl MerchantRunner for LiveMerchant {
    fn sell(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        let layout = match self.env.layout(&self.cancel, &self.config.calibration) {
            Ok(layout) => layout,
            Err(reason) => return refused(reason),
        };
        let monitor = self.env.monitor(&self.cancel);
        let pause = self.env.pause(&self.cancel, self.config.step_delay_s);
        let tab_mapping = self.config.tab_mapping.clone();
        let runner = MerchantSaleRunner::new(Arc::clone(&self.env.input), layout, Arc::clone(&self.game), tab_mapping, self.cancel.clone(), pause)
            .with_safety(monitor.clone());
        MonitoredRunner::new(runner, monitor).sell(entries, dry_run, on_progress)
    }
}

/// The game's resolution from its window's client area: the `"WxH"` key [`LiveConfig::calibration`]
/// entries are saved under. `None` when the game isn't running (or is minimized).
pub fn game_resolution() -> Option<(u32, u32)> {
    let area = input::client_area(input::find_game_window()?)?;
    (area.width > 0 && area.height > 0).then(|| (area.width.unsigned_abs(), area.height.unsigned_abs()))
}
