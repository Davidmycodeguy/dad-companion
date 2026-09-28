//! Stopping the run the moment it is no longer safe to drive the mouse: the game lost focus, or the
//! player moved the mouse. Port of the OS-polling half of `sort_safety.py`; the focus grace period
//! itself is [`SortSafetyMonitor`]'s.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use input::{Cancel, InputSink};

use super::machine::Machine;
use super::report::StopReason;
use crate::plan::SortSafetyMonitor;

/// Decides whether the cursor is where the sorter last put it.
///
/// Python compared the cursor between moves and wanted three deviations in a row, but reset the
/// count after every move, so it could never fire. Absolute `SendInput` moves also snap the cursor
/// back on every step, which hides the player's hand while the sorter is moving it. So this looks
/// only while the cursor should be still (at least `settle` after the sorter's last move), and then
/// any drift beyond `tolerance` is the player.
#[derive(Debug, Clone)]
pub struct InterferenceGuard {
    tolerance: i32,
    settle: Duration,
    expected: Option<(i32, i32)>,
    moved_at: Option<Instant>,
}

impl InterferenceGuard {
    /// How long after one of its own moves the sorter trusts the cursor to have arrived.
    pub const SETTLE: Duration = Duration::from_millis(20);

    pub fn new(tolerance: i32, settle: Duration) -> Self {
        InterferenceGuard { tolerance, settle, expected: None, moved_at: None }
    }

    /// Drift allowed for a grid of `jump`-pixel cells: 40% of a cell, at least 12 px.
    pub fn tolerance_for(jump: f64) -> i32 {
        ((jump * 0.4).round() as i32).max(12)
    }

    /// The sorter just put the cursor at `at`.
    pub fn commanded(&mut self, at: (i32, i32), now: Instant) {
        self.expected = Some(at);
        self.moved_at = Some(now);
    }

    /// Starts watching from `at`, as if the sorter had put the cursor there long ago.
    pub fn baseline(&mut self, at: (i32, i32)) {
        self.expected = Some(at);
        self.moved_at = None;
    }

    /// Whether the cursor at `actual` fits the sorter's last move; `false` means the player moved it.
    pub fn observe(&self, actual: (i32, i32), now: Instant) -> bool {
        let Some((x, y)) = self.expected else { return true };
        if self.moved_at.is_some_and(|at| now.saturating_duration_since(at) < self.settle) {
            return true;
        }
        (actual.0 - x).abs().max((actual.1 - y).abs()) <= self.tolerance
    }
}

struct State {
    armed: bool,
    focus: SortSafetyMonitor,
    guard: InterferenceGuard,
    tripped: Option<StopReason>,
}

/// One run's safety state, shared by the run loop, its [`GuardedSink`] and the [`Watchdog`].
/// Nothing is checked until [`Safety::arm`]: before the game is in front, it having no focus is
/// expected.
pub struct Safety {
    state: Mutex<State>,
}

impl Safety {
    pub fn new(guard: InterferenceGuard) -> Self {
        Safety { state: Mutex::new(State { armed: false, focus: SortSafetyMonitor::new(), guard, tripped: None }) }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts checking focus and the mouse, with the cursor's current position as the baseline.
    pub fn arm(&self, cursor: Option<(i32, i32)>) {
        let mut state = self.lock();
        state.armed = true;
        if let Some(at) = cursor {
            state.guard.baseline(at);
        }
    }

    /// Stops checking (the run is over).
    pub fn disarm(&self) {
        self.lock().armed = false;
    }

    pub fn tripped(&self) -> Option<StopReason> {
        self.lock().tripped.clone()
    }

    /// Records why the run stops; the first reason wins.
    pub fn trip(&self, reason: StopReason) {
        self.lock().tripped.get_or_insert(reason);
    }

    /// Runs `send` (one cursor move to `at`) and records the move as the sorter's own, with no
    /// [`Self::poll`] in between: a poll never sees the cursor half-way through the bookkeeping.
    pub fn commanded_move(&self, at: (i32, i32), send: impl FnOnce()) {
        let mut state = self.lock();
        send();
        state.guard.commanded(at, Instant::now());
    }

    /// One check: `read` returns (game has focus, cursor position) and runs under the same lock as
    /// [`Self::commanded_move`]. Returns the reason once the run must stop.
    pub fn poll(&self, now: Instant, read: impl FnOnce() -> (bool, Option<(i32, i32)>)) -> Option<StopReason> {
        let mut state = self.lock();
        if state.tripped.is_some() || !state.armed {
            return state.tripped.clone();
        }
        let (focused, cursor) = read();
        if !state.focus.poll_focus(now, focused) {
            state.tripped = Some(StopReason::FocusLost);
        } else if cursor.is_some_and(|at| !state.guard.observe(at, now)) {
            state.tripped = Some(StopReason::MouseMoved);
        }
        state.tripped.clone()
    }
}

/// An [`InputSink`] that stops pressing and moving once the run is cancelled, and tells
/// [`Safety`] where it put the cursor. Releases always go through, so a stopped drag never leaves
/// a mouse button or Alt held down.
pub struct GuardedSink<'a> {
    inner: &'a mut dyn InputSink,
    safety: &'a Safety,
    cancel: &'a Cancel,
}

impl<'a> GuardedSink<'a> {
    pub fn new(inner: &'a mut dyn InputSink, safety: &'a Safety, cancel: &'a Cancel) -> Self {
        GuardedSink { inner, safety, cancel }
    }
}

impl InputSink for GuardedSink<'_> {
    fn move_to(&mut self, x: i32, y: i32) {
        if self.cancel.is_cancelled() {
            return;
        }
        let inner = &mut *self.inner;
        self.safety.commanded_move((x, y), || inner.move_to(x, y));
    }

    fn mouse_down(&mut self) {
        if !self.cancel.is_cancelled() {
            self.inner.mouse_down();
        }
    }

    fn mouse_up(&mut self) {
        self.inner.mouse_up();
    }

    fn key_event(&mut self, vk: u16, key_up: bool) {
        if key_up || !self.cancel.is_cancelled() {
            self.inner.key_event(vk, key_up);
        }
    }

    fn cursor_position(&self) -> (i32, i32) {
        self.inner.cursor_position()
    }
}

/// Polls [`Safety`] on its own thread until dropped or stopped, cancelling the run as soon as a
/// check fails. The run loop polls at every checkpoint too; this covers the waits inside a drag.
pub struct Watchdog {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watchdog {
    /// Stops polling and waits for the thread to finish.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.halt();
    }
}

/// Starts a [`Watchdog`] polling every `interval`.
pub fn spawn_watchdog(safety: Arc<Safety>, machine: Arc<dyn Machine>, cancel: Cancel, interval: Duration) -> std::io::Result<Watchdog> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let thread = std::thread::Builder::new().name("sort-safety".into()).spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            if safety.poll(Instant::now(), || (machine.game_focused(), machine.cursor())).is_some() {
                cancel.cancel();
                return;
            }
            std::thread::sleep(interval);
        }
    })?;
    Ok(Watchdog { stop, thread: Some(thread) })
}
