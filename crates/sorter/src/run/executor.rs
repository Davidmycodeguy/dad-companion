//! The run loop: bring the game forward, make sure the right stash is on screen, then drag every
//! planned move and confirm it before the next. Port of `StashSorter._replay_planned_moves` plus
//! the window and tab handling of `StashManager.sort_stash`.

mod step;

use std::time::{Duration, Instant};

use input::{click_point, move_mouse_smooth, Cancel, DragState, InputSink, Point, SmoothMoveOptions};

use super::layout_check::{check_layout, CellPatch, LayoutVerdict};
use super::machine::Machine;
use super::report::{RunObserver, RunPhase, RunReport, StopReason};
use super::safety::{GuardedSink, Safety};
use super::screen_map::ScreenMap;
use super::speed::AdaptiveSpeed;
use super::steps::{Board, RunSteps};
use crate::plan::Cell;

/// Waits between the run's actions: [`Timing::GAME`] in the game, [`Timing::INSTANT`] in tests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// Longest wait for the game to take focus after being brought forward.
    pub focus_timeout: Duration,
    /// After the game has focus, before the first capture (longer for exclusive fullscreen).
    pub after_focus: Duration,
    /// After clicking a stash tab, before checking it.
    pub after_tab: Duration,
    /// After parking the cursor, before a "before" capture: lets a tooltip close.
    pub before_capture: Duration,
    /// After a drag and parking, before the "after" capture. Python: `_RENDER_DELAY` (80 ms),
    /// plus time for the dropped item's tooltip to close.
    pub after_drag: Duration,
    /// Before looking again at a move that did not show up the first time.
    pub recheck: Duration,
    /// Pace the cursor's travel between cells like a hand would (off in tests).
    pub paced_travel: bool,
}

impl Timing {
    pub const GAME: Timing = Timing {
        focus_timeout: Duration::from_secs(3),
        after_focus: Duration::from_millis(400),
        after_tab: Duration::from_millis(450),
        before_capture: Duration::from_millis(80),
        after_drag: Duration::from_millis(120),
        recheck: Duration::from_millis(350),
        paced_travel: true,
    };

    pub const INSTANT: Timing = Timing {
        focus_timeout: Duration::ZERO,
        after_focus: Duration::ZERO,
        after_tab: Duration::ZERO,
        before_capture: Duration::ZERO,
        after_drag: Duration::ZERO,
        recheck: Duration::ZERO,
        paced_travel: false,
    };
}

/// How often a wait looks at safety and the cancel flag (the same 10 ms `Cancel::sleep` uses).
const POLL: Duration = Duration::from_millis(10);
/// How often the game is asked whether it has focus while the run waits for it.
const FOCUS_POLL: Duration = Duration::from_millis(50);
/// Steps of the cursor's travel between parking spots and targets, and the pause range per step
/// (`move_mouse_smooth` never pauses less than 3 ms).
const TRAVEL_STEPS: u32 = 14;
const TRAVEL_STEP_DELAY: (Duration, Duration) = (Duration::from_millis(3), Duration::from_millis(5));

/// Everything about the screen a run needs besides the plan.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub map: ScreenMap,
    /// Where to click to open the stash's tab, when a tab is known for it (see [`super::tab_index`]).
    pub tab_point: Option<(i32, i32)>,
    /// The player's drag delay. Python: the `sortSpeed` setting.
    pub move_delay: Duration,
    pub timing: Timing,
}

/// Performs `steps` in the game with `sink` and reports what was done. Stops at the first move it
/// can't confirm, on cancel, or as soon as `safety` trips. `safety` should be fresh for each run;
/// it is armed once the game is in front and disarmed on return.
pub fn run_sort(
    sink: &mut dyn InputSink,
    machine: &dyn Machine,
    cancel: &Cancel,
    safety: &Safety,
    config: &RunConfig,
    steps: &RunSteps,
    observer: &mut dyn RunObserver,
) -> RunReport {
    let mut run = Run {
        sink: GuardedSink::new(sink, safety, cancel),
        machine,
        cancel,
        safety,
        config,
        observer,
        board: steps.initial().clone(),
        drag_state: DragState::new(),
        speed: AdaptiveSpeed::new(config.move_delay),
        report: RunReport { total: steps.len(), ..RunReport::default() },
        landing: None,
    };
    if !steps.is_empty() {
        let result = run.prepare().and_then(|()| run.move_all(steps));
        run.report.stop = result.err();
    }
    safety.disarm();
    run.report
}

struct Run<'a> {
    sink: GuardedSink<'a>,
    machine: &'a dyn Machine,
    cancel: &'a Cancel,
    safety: &'a Safety,
    config: &'a RunConfig,
    observer: &'a mut dyn RunObserver,
    board: Board,
    drag_state: DragState,
    speed: AdaptiveSpeed,
    report: RunReport,
    /// What the last move left on screen, looked at again before the next drag.
    landing: Option<step::Landing>,
}

impl Run<'_> {
    /// Stops here if safety tripped or the run was cancelled.
    fn checkpoint(&mut self) -> Result<(), StopReason> {
        let machine = self.machine;
        if let Some(reason) = self.safety.poll(Instant::now(), || (machine.game_focused(), machine.cursor())) {
            self.cancel.cancel();
            return Err(reason);
        }
        if self.cancel.is_cancelled() {
            return Err(stop_reason(self.safety));
        }
        Ok(())
    }

    /// Waits `duration`, checking safety every few milliseconds.
    fn wait(&mut self, duration: Duration) -> Result<(), StopReason> {
        let deadline = Instant::now() + duration;
        loop {
            self.checkpoint()?;
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            std::thread::sleep((deadline - now).min(POLL));
        }
    }

    /// Moves the cursor to `to` in small steps.
    fn travel(&mut self, to: (i32, i32)) -> Result<(), StopReason> {
        let from = self.sink.cursor_position();
        if from != to {
            let options = SmoothMoveOptions {
                steps: TRAVEL_STEPS,
                min_delay: TRAVEL_STEP_DELAY.0,
                max_delay: TRAVEL_STEP_DELAY.1,
                no_delay: !self.config.timing.paced_travel,
            };
            let path = ((f64::from(from.0), f64::from(from.1)), (f64::from(to.0), f64::from(to.1)));
            move_mouse_smooth(&mut self.sink, self.cancel, path.0, path.1, options).map_err(|err| input_error(self.safety, err))?;
        }
        self.checkpoint()
    }

    /// Brings the game forward, waits for focus, arms safety, and makes sure the stash on screen
    /// is the plan's, clicking its tab once if it is not.
    fn prepare(&mut self) -> Result<(), StopReason> {
        self.observer.phase(RunPhase::FocusingGame);
        self.checkpoint()?;
        self.machine.bring_game_forward(&mut self.sink)?;
        self.wait_for_focus()?;
        self.wait(self.config.timing.after_focus)?;
        self.safety.arm(self.machine.cursor());

        self.observer.phase(RunPhase::CheckingStash);
        match self.check_stash()? {
            LayoutVerdict::Match => return Ok(()),
            LayoutVerdict::Unreadable => return Err(StopReason::LayoutUnreadable),
            LayoutVerdict::Mismatch => {}
        }
        let Some(tab) = self.config.tab_point else { return Err(StopReason::LayoutMismatch) };
        self.observer.phase(RunPhase::OpeningTab);
        self.travel(tab)?;
        click_point(&mut self.sink, self.cancel, Point::new(tab.0, tab.1)).map_err(|err| input_error(self.safety, err))?;
        self.wait(self.config.timing.after_tab)?;
        self.observer.phase(RunPhase::CheckingStash);
        match self.check_stash()? {
            LayoutVerdict::Match => Ok(()),
            LayoutVerdict::Unreadable => Err(StopReason::LayoutUnreadable),
            LayoutVerdict::Mismatch => Err(StopReason::LayoutMismatch),
        }
    }

    fn wait_for_focus(&mut self) -> Result<(), StopReason> {
        let deadline = Instant::now() + self.config.timing.focus_timeout;
        loop {
            if self.machine.game_focused() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(StopReason::GameNotFocused);
            }
            self.wait(FOCUS_POLL)?;
        }
    }

    /// Parks the cursor outside both grids, captures them, and compares every cell with the board.
    fn check_stash(&mut self) -> Result<LayoutVerdict, StopReason> {
        let config = self.config;
        let map = &config.map;
        self.travel(map.neutral_point())?;
        self.wait(self.config.timing.before_capture)?;
        let mut patches = Vec::new();
        for inventory_id in [map.stash_id(), map.bag_id()] {
            let (Some(region), Some((width, height))) = (map.grid_region(inventory_id), map.size(inventory_id)) else { continue };
            let frame = self.machine.grab(region).map_err(StopReason::Capture)?;
            for y in 0..height {
                for x in 0..width {
                    let cell = Cell::new(x, y);
                    let Some((left, top, right, bottom)) = map.cell_interior(inventory_id, cell) else { continue };
                    patches.push(CellPatch {
                        in_stash: inventory_id == map.stash_id(),
                        expected_occupied: self.board.is_occupied(inventory_id, cell),
                        pixels: frame.crop(left, top, right, bottom),
                    });
                }
            }
        }
        let check = check_layout(&patches);
        self.observer.layout_checked(&check);
        self.checkpoint()?;
        Ok(check.verdict)
    }
}

/// Why the run was cancelled: what safety saw, or the player's stop.
fn stop_reason(safety: &Safety) -> StopReason {
    safety.tripped().unwrap_or(StopReason::Cancelled)
}

fn input_error(safety: &Safety, err: input::Error) -> StopReason {
    match err {
        input::Error::Cancelled => stop_reason(safety),
        other => StopReason::Input(other.to_string()),
    }
}
