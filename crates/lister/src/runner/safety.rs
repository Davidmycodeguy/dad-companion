//! Watching for the player taking over while a runner clicks.
//!
//! [`SafetyCheck`] is what the runners ask between steps (DnDTools' runner-side `Safety`
//! protocol); [`SafetyMonitor`] is the live implementation, a port of DnDTools' `SortSafetyMonitor`
//! (`src/models/sort_safety.py`). It also implements [`crate::job::Safety`], so
//! [`crate::job::MonitoredRunner`] starts it before each runner action and stops it after.
//!
//! Two checks run side by side:
//! - **Window focus**, on a background thread: polled every [`FOCUS_POLL_INTERVAL`]; once the game
//!   has been out of focus for [`FOCUS_LOSS_GRACE_S`], the run is cancelled. A brief alt-tab that
//!   returns within the grace period does not cancel it.
//! - **Mouse interference**, at the runner's checkpoints: each [`SafetyCheck::checkpoint`] compares
//!   the cursor with where it was at the previous checkpoint (or the last
//!   [`SafetyCheck::snapshot_position`]); [`MOUSE_DEVIATION_STRIKES`] deviations in a row of more
//!   than [`MOUSE_DEVIATION_THRESHOLD_PX`] cancel the run.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{MOUSE_REASON, UNFOCUSED_REASON};
use crate::job::{CancelToken, Safety};

/// Seconds between background focus checks.
pub const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// How long (seconds) the game may lose focus before the run is cancelled: short enough to react
/// quickly, long enough to survive a brief taskbar flash or overlay tooltip.
pub const FOCUS_LOSS_GRACE_S: f64 = 0.6;
/// Pixels (on either axis) the cursor must move from the expected position to count as a strike.
pub const MOUSE_DEVIATION_THRESHOLD_PX: u32 = 120;
/// Deviating checkpoints in a row that cancel the run.
pub const MOUSE_DEVIATION_STRIKES: u32 = 3;
/// [`SafetyCheck::reason`] if the focus watcher thread could not even be started; nothing is
/// clicked without it.
pub const NO_WATCHER_REASON: &str = "couldn't start watching the game window";

/// What a runner asks between steps: has the player taken over?
pub trait SafetyCheck: Send + Sync {
    /// Why the run was stopped for safety ([`UNFOCUSED_REASON`], [`MOUSE_REASON`]), if it was.
    fn reason(&self) -> Option<String>;
    /// `false` once the run must stop.
    fn checkpoint(&self) -> bool;
    /// Records where the cursor rests after the runner's own move, as the next checkpoint's
    /// baseline.
    fn snapshot_position(&self);
}

/// Never stops anything (DnDTools' `NullSafety`, the default when no monitor is given).
#[derive(Debug, Clone, Copy, Default)]
pub struct NullSafety;

impl SafetyCheck for NullSafety {
    fn reason(&self) -> Option<String> {
        None
    }

    fn checkpoint(&self) -> bool {
        true
    }

    fn snapshot_position(&self) {}
}

/// The two things [`SafetyMonitor`] reads from the desktop, as a trait so tests can fake them.
pub trait Desktop: Send + Sync {
    /// The cursor position, or `None` if it can't be read right now.
    fn cursor_position(&self) -> Option<(i32, i32)>;
    /// True while the game (or an in-game overlay above it) has focus.
    fn game_has_focus(&self) -> bool;
}

/// Decides when a lost focus has lasted long enough to stop the run: the bookkeeping of
/// `SortSafetyMonitor._focus_loop`, kept apart from its thread so it can be tested with plain
/// numbers.
#[derive(Debug, Clone, Default)]
pub struct FocusGrace {
    lost_since: Option<f64>,
}

impl FocusGrace {
    /// Feeds one poll (`now` in seconds); `true` once the game has been out of focus for at least
    /// [`FOCUS_LOSS_GRACE_S`], counted from the first poll that saw it unfocused.
    pub fn observe(&mut self, focused: bool, now: f64) -> bool {
        if focused {
            self.lost_since = None;
            return false;
        }
        match self.lost_since {
            None => {
                self.lost_since = Some(now);
                false
            }
            Some(since) => now - since >= FOCUS_LOSS_GRACE_S,
        }
    }
}

/// Where the cursor should be, the strikes against it, and why the monitor stopped the run.
#[derive(Debug, Default)]
struct Tracking {
    expected: Option<(i32, i32)>,
    strikes: u32,
    reason: Option<String>,
}

/// Locks `mutex` even if a panicking thread poisoned it: the monitor's data stays meaningful (a
/// position, a counter, a reason), and the monitor must keep working to keep the player safe.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How long `stop` waits for the focus watcher to finish (`thread.join(timeout=1.0)`).
const STOP_WAIT: Duration = Duration::from_secs(1);
const STOP_POLL: Duration = Duration::from_millis(2);

/// Asks one focus watcher to stop, waking it at once. Each watcher has its own, so a watcher that
/// `stop` gave up waiting for can never outlive into (and trip) a later start.
#[derive(Default)]
struct StopSignal {
    raised: Mutex<bool>,
    wake: Condvar,
}

impl StopSignal {
    fn raise(&self) {
        *lock(&self.raised) = true;
        self.wake.notify_all();
    }

    fn is_raised(&self) -> bool {
        *lock(&self.raised)
    }

    /// Sleeps up to `timeout`, waking early once raised; true when raised.
    fn wait(&self, timeout: Duration) -> bool {
        let raised = lock(&self.raised);
        let (raised, _) = self.wake.wait_timeout_while(raised, timeout, |raised| !*raised).unwrap_or_else(PoisonError::into_inner);
        *raised
    }
}

/// The running focus watcher.
struct Watcher {
    thread: JoinHandle<()>,
    stop: Arc<StopSignal>,
}

/// Watches window focus and mouse interference while a runner action runs, cancelling the run's
/// token (with a [`SafetyCheck::reason`]) when the player takes over. Port of DnDTools'
/// `SortSafetyMonitor`.
///
/// ```text
/// monitor.start();                       // MonitoredRunner does this around each action
/// for step in steps {
///     if !monitor.checkpoint() { break; }
///     do_step(step);
///     monitor.snapshot_position();
/// }
/// monitor.stop();
/// ```
pub struct SafetyMonitor {
    cancel: CancelToken,
    desktop: Arc<dyn Desktop>,
    tracking: Arc<Mutex<Tracking>>,
    watcher: Mutex<Option<Watcher>>,
}

impl SafetyMonitor {
    /// A stopped monitor that cancels `cancel` when the player takes over.
    pub fn new(cancel: CancelToken, desktop: Arc<dyn Desktop>) -> Self {
        SafetyMonitor { cancel, desktop, tracking: Arc::default(), watcher: Mutex::new(None) }
    }

    fn trip(&self, reason: &str) {
        lock(&self.tracking).reason = Some(reason.to_string());
        self.cancel.cancel();
    }
}

/// The focus watcher thread's loop (`SortSafetyMonitor._focus_loop`).
fn watch_focus(stop: &StopSignal, tracking: &Mutex<Tracking>, cancel: &CancelToken, desktop: &dyn Desktop) {
    let started = Instant::now();
    let mut grace = FocusGrace::default();
    loop {
        if stop.wait(FOCUS_POLL_INTERVAL) || cancel.is_cancelled() {
            return;
        }
        let focused = desktop.game_has_focus();
        // Stopped while asking Windows: the action is over, nothing left to guard.
        if stop.is_raised() {
            return;
        }
        if grace.observe(focused, started.elapsed().as_secs_f64()) {
            lock(tracking).reason = Some(UNFOCUSED_REASON.to_string());
            cancel.cancel();
            return;
        }
    }
}

impl Safety for SafetyMonitor {
    /// Starts watching focus (a no-op while already watching) with a clean slate: no reason, no
    /// strikes, no expected cursor position.
    fn start(&self) {
        let mut watcher = lock(&self.watcher);
        if watcher.as_ref().is_some_and(|running| !running.thread.is_finished()) {
            return;
        }
        *lock(&self.tracking) = Tracking::default();
        let stop = Arc::new(StopSignal::default());
        let (signal, tracking, cancel, desktop) =
            (Arc::clone(&stop), Arc::clone(&self.tracking), self.cancel.clone(), Arc::clone(&self.desktop));
        let spawned = std::thread::Builder::new()
            .name("SafetyMonitor".to_string())
            .spawn(move || watch_focus(&signal, &tracking, &cancel, &*desktop));
        match spawned {
            Ok(thread) => *watcher = Some(Watcher { thread, stop }),
            // Nothing may be clicked unwatched: stop the run before its first step instead.
            Err(_) => self.trip(NO_WATCHER_REASON),
        }
    }

    /// Stops the focus watcher, waiting up to a second for it to finish (it wakes at once when
    /// asked; one stuck in a Windows call is left to exit on its own).
    fn stop(&self) {
        let Some(watcher) = lock(&self.watcher).take() else {
            return;
        };
        watcher.stop.raise();
        let deadline = Instant::now() + STOP_WAIT;
        while !watcher.thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(STOP_POLL);
        }
        if watcher.thread.is_finished() {
            // A panic in the watcher has nothing left to clean up; the run is over either way.
            let _ = watcher.thread.join();
        }
    }
}

impl SafetyCheck for SafetyMonitor {
    fn reason(&self) -> Option<String> {
        lock(&self.tracking).reason.clone()
    }

    /// Records the cursor and checks it against the previous baseline. `false` once the run was
    /// cancelled (by anyone) or this checkpoint is the last of [`MOUSE_DEVIATION_STRIKES`]
    /// deviations in a row. A cursor that can't be read never cancels.
    fn checkpoint(&self) -> bool {
        if self.cancel.is_cancelled() {
            return false;
        }
        let Some(current) = self.desktop.cursor_position() else {
            return true;
        };
        let mut tracking = lock(&self.tracking);
        // Always updated, so the next checkpoint has a fresh baseline even without a move between.
        let Some(expected) = tracking.expected.replace(current) else {
            return true;
        };
        let deviation = current.0.abs_diff(expected.0).max(current.1.abs_diff(expected.1));
        if deviation <= MOUSE_DEVIATION_THRESHOLD_PX {
            tracking.strikes = 0;
            return true;
        }
        tracking.strikes += 1;
        if tracking.strikes < MOUSE_DEVIATION_STRIKES {
            return true;
        }
        drop(tracking);
        self.trip(MOUSE_REASON);
        false
    }

    fn snapshot_position(&self) {
        if let Some(position) = self.desktop.cursor_position() {
            let mut tracking = lock(&self.tracking);
            tracking.expected = Some(position);
            tracking.strikes = 0;
        }
    }
}
