//! The run thread: where the grids are on this screen, then the sort itself with the real mouse,
//! with safety polling, learning and the page's status kept up to date.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use appdata::Settings;
use input::{Cancel, CalibrationOverride, WindowsInput};
use sorter::plan::SortPlan;
use sorter::run::{
    layout_for, plan_metrics, run_sort, session_features, spawn_watchdog, tab_index, Desktop, GridGeometry, InterferenceGuard, LearningRecorder,
    Machine, RunConfig, RunReport, RunSteps, Safety, ScreenMap, StopReason, Timing,
};
use state::stash::BAG;
use state::Character;

use super::learning::Learning;
use super::options::{self, SortOptions};
use super::status::{lock as lock_status, StatusObserver, SorterStatus};
use super::{lock, Shared, Sorter};
use crate::input_lock::InputGuard;

/// How often the safety watchdog looks at focus and the cursor during waits inside a drag.
const WATCH_INTERVAL: Duration = Duration::from_millis(25);
/// Exclusive fullscreen takes longer to settle after the game comes to the front (Python waited 1 s).
const EXCLUSIVE_FULLSCREEN_SETTLE: Duration = Duration::from_secs(1);
/// `FullscreenMode` of exclusive fullscreen in the game's settings file.
const EXCLUSIVE_FULLSCREEN: u32 = 0;

/// Everything one run needs, planned and checked before the thread starts.
pub struct RunJob {
    pub character: Character,
    pub stash_id: u32,
    pub stash_label: String,
    pub options: SortOptions,
    pub plan: SortPlan,
    pub steps: RunSteps,
    pub config: RunConfig,
    /// The character data the plan was built from (see `Shared::awaiting_refresh`).
    pub fingerprint: u64,
    pub learning_enabled: bool,
}

/// Where the grids and the stash's tab are on this screen. The game's resolution is the one set
/// in Settings, else the game window's client area (its real render size in every window mode),
/// else the game's settings file.
pub fn screen_config(settings: &Settings, character: &Character, stash_id: u32) -> RunConfig {
    let client = input::find_game_window().and_then(input::client_area);
    let ini = input::game_config_path().and_then(|path| std::fs::read_to_string(path).ok());
    let resolution = options::saved_resolution(settings)
        .or_else(|| client.filter(|area| area.width > 0 && area.height > 0).map(|area| (area.width as u32, area.height as u32)))
        .or_else(|| ini.as_deref().and_then(input::resolution_from_ini))
        .unwrap_or(input::BASE_RESOLUTION);
    let calibration = settings.get::<CalibrationOverride>(options::CALIBRATION_KEY);
    let layout = layout_for(resolution, client, calibration.as_ref());
    let mapping = settings.get::<Vec<i64>>(options::TAB_MAPPING_KEY);
    let tab_point = tab_index(mapping.as_deref(), &character.storages, stash_id)
        .and_then(|index| layout.stash_tab_positions().get(index).map(|point| (point.x, point.y)));
    let exclusive = ini.as_deref().and_then(input::window_mode_from_ini) == Some(EXCLUSIVE_FULLSCREEN);
    let timing = Timing { after_focus: if exclusive { EXCLUSIVE_FULLSCREEN_SETTLE } else { Timing::GAME.after_focus }, ..Timing::GAME };
    RunConfig {
        map: ScreenMap::new(GridGeometry::from_layout(&layout), stash_id, BAG),
        tab_point,
        move_delay: options::move_delay(settings),
        timing,
    }
}

/// Marks the sorter running and starts the run thread, which holds `guard` until it ends.
pub fn spawn(sorter: &Sorter, guard: InputGuard, job: RunJob) -> Result<(), String> {
    let cancel = Cancel::new();
    {
        let mut status = lock_status(&sorter.shared.status);
        *status = SorterStatus {
            state: "running",
            character_id: Some(job.character.id.clone()),
            stash_id: Some(job.stash_id),
            stash_label: Some(job.stash_label.clone()),
            total: job.steps.len(),
            hotkey: guard.stop_hotkey_active(),
            ..SorterStatus::idle()
        };
    }
    // The Stop button and Ctrl+F12 work from this moment on, before the thread has even started.
    let tolerance = InterferenceGuard::tolerance_for(job.config.map.jump());
    let safety = Arc::new(Safety::new(InterferenceGuard::new(tolerance, InterferenceGuard::SETTLE)));
    {
        let (safety, cancel) = (Arc::clone(&safety), cancel.clone());
        guard.on_stop(move || {
            safety.trip(StopReason::Cancelled);
            cancel.cancel();
        });
    }
    let (shared, learning) = (Arc::clone(&sorter.shared), sorter.learning.clone());
    let started = std::thread::Builder::new()
        .name("stash-sorter".into())
        .spawn(move || run(&shared, learning.as_deref(), guard, job, cancel, safety));
    started.map(|_| ()).map_err(|err| {
        *lock_status(&sorter.shared.status) = SorterStatus::idle();
        format!("The sorter could not start: {err}")
    })
}

/// Closes out a run the thread didn't finish (a panic): the page must never show a run that isn't
/// going, and the stash may have changed, so the next sort waits for fresh data.
struct Unfinished<'a> {
    shared: &'a Shared,
    character_id: String,
    fingerprint: u64,
    stash_id: u32,
}

impl Drop for Unfinished<'_> {
    fn drop(&mut self) {
        let mut status = lock_status(&self.shared.status);
        if !status.is_running() {
            return;
        }
        status.state = "stopped";
        status.phase = None;
        status.current = None;
        status.stopped_reason = Some("The sorter stopped unexpectedly. Check the stash in the game before sorting again.".into());
        status.finished_at = Some(unix_now());
        drop(status);
        lock(&self.shared.awaiting_refresh).insert(self.character_id.clone(), (self.fingerprint, self.stash_id));
    }
}

fn run(shared: &Shared, learning: Option<&Learning>, guard: InputGuard, job: RunJob, cancel: Cancel, safety: Arc<Safety>) {
    let _unfinished = Unfinished { shared, character_id: job.character.id.clone(), fingerprint: job.fingerprint, stash_id: job.stash_id };
    let machine: Arc<dyn Machine> = Arc::new(Desktop);
    let watchdog = spawn_watchdog(Arc::clone(&safety), Arc::clone(&machine), cancel.clone(), WATCH_INTERVAL)
        .inspect_err(|err| log::warn!("safety watchdog could not start, checks run between moves only: {err}"))
        .ok();
    let mut observer = StatusObserver::new(Arc::clone(&shared.status));
    let mut mouse = WindowsInput;
    let (report, session_id) = match learning {
        Some(learning) => run_learning(learning, &job, &mut mouse, machine.as_ref(), &cancel, &safety, &mut observer),
        None => (run_sort(&mut mouse, machine.as_ref(), &cancel, &safety, &job.config, &job.steps, &mut observer), None),
    };
    drop(watchdog);
    log::info!("sort of {} ended: {} of {} moves, {} verified, stop: {:?}", job.stash_label, report.done, report.total, report.verified, report.stop);
    if report.started > 0 {
        lock(&shared.awaiting_refresh).insert(job.character.id.clone(), (job.fingerprint, job.stash_id));
    }
    {
        let mut status = lock_status(&shared.status);
        status.finish(&report, unix_now());
        status.session_id = session_id;
    }
    if let Some(learning) = learning {
        learning.train_if_due(job.learning_enabled);
    }
    drop(guard);
}

/// The run inside a learning session: features and plan metrics first, one outcome per move, the
/// session's result last. Returns the session id for the player's feedback.
fn run_learning(
    learning: &Learning,
    job: &RunJob,
    mouse: &mut WindowsInput,
    machine: &dyn Machine,
    cancel: &Cancel,
    safety: &Safety,
    observer: &mut StatusObserver,
) -> (RunReport, Option<String>) {
    let features = session_features(job.steps.initial());
    let mut handle = learning.observer.begin_session(Some(job.character.id.clone()), Some(job.stash_id), job.options.pack, job.options.stack, features);
    for (key, value) in plan_metrics(&job.steps, &job.plan) {
        handle.set_metric(key, value);
    }
    learning.register_plan(handle.session_id(), &job.plan, &job.character);
    let report = {
        let mut recorder = LearningRecorder::new(&mut handle, Some(learning.store.as_ref()), observer);
        run_sort(mouse, machine, cancel, safety, &job.config, &job.steps, &mut recorder)
    };
    let cancelled = report.stop.as_ref().is_some_and(StopReason::is_cancel);
    let summary = handle.finalize(report.is_complete(), cancelled, report.stop.as_ref().map(|reason| reason.code().to_string()));
    (report, Some(summary.session_id))
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}
