//! The safety monitor (DnDTools' `SortSafetyMonitor`, which had no tests of its own), driven by a
//! fake desktop.

#[path = "runner_fakes.rs"]
mod fakes;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fakes::FakeDesktop;
use lister::job::{CancelToken, Safety};
use lister::runner::safety::{FocusGrace, FOCUS_LOSS_GRACE_S};
use lister::runner::{Desktop, SafetyCheck, SafetyMonitor};

fn monitor() -> (SafetyMonitor, Arc<FakeDesktop>, CancelToken) {
    let (desktop, cancel) = (FakeDesktop::new(), CancelToken::new());
    (SafetyMonitor::new(cancel.clone(), desktop.clone()), desktop, cancel)
}

/// Checkpoints with the cursor at each of `positions` in turn.
fn checkpoints(monitor: &SafetyMonitor, desktop: &FakeDesktop, positions: &[(i32, i32)]) -> Vec<bool> {
    positions
        .iter()
        .map(|&position| {
            desktop.set_cursor(Some(position));
            monitor.checkpoint()
        })
        .collect()
}

fn wait_until(deadline: Duration, done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

#[test]
fn focus_loss_trips_only_after_the_grace_period() {
    let mut grace = FocusGrace::default();
    assert!(!grace.observe(false, 0.0));
    assert!(!grace.observe(false, FOCUS_LOSS_GRACE_S - 0.1));
    assert!(grace.observe(false, FOCUS_LOSS_GRACE_S));
}

#[test]
fn a_brief_alt_tab_restarts_the_grace_period() {
    let mut grace = FocusGrace::default();
    assert!(!grace.observe(false, 0.0));
    assert!(!grace.observe(true, 0.3));
    assert!(!grace.observe(false, 0.5));
    assert!(!grace.observe(false, 0.5 + FOCUS_LOSS_GRACE_S - 0.1));
    assert!(grace.observe(false, 0.5 + FOCUS_LOSS_GRACE_S));
}

#[test]
fn three_deviating_checkpoints_in_a_row_cancel_the_run() {
    let (monitor, desktop, cancel) = monitor();
    let results = checkpoints(&monitor, &desktop, &[(0, 0), (200, 0), (400, 0), (600, 0)]);
    assert_eq!(results, [true, true, true, false]);
    assert!(cancel.is_cancelled());
    assert_eq!(monitor.reason().as_deref(), Some("mouse_interference"));
}

#[test]
fn a_checkpoint_near_the_last_one_clears_the_strikes() {
    let (monitor, desktop, cancel) = monitor();
    // 200 px (strike), 50 px (clears), then two strikes: not three in a row.
    let results = checkpoints(&monitor, &desktop, &[(0, 0), (200, 0), (250, 0), (0, 300), (0, 0)]);
    assert_eq!(results, [true; 5]);
    assert!(!cancel.is_cancelled());
    assert_eq!(monitor.reason(), None);
}

#[test]
fn snapshot_position_is_the_next_baseline_and_clears_the_strikes() {
    let (monitor, desktop, cancel) = monitor();
    checkpoints(&monitor, &desktop, &[(0, 0), (200, 0), (400, 0)]);
    // The runner's own move lands far away; the snapshot makes it the new baseline.
    desktop.set_cursor(Some((1500, 900)));
    monitor.snapshot_position();
    assert_eq!(checkpoints(&monitor, &desktop, &[(1510, 905), (1700, 905), (1900, 905)]), [true; 3]);
    assert!(!cancel.is_cancelled());
}

#[test]
fn an_unreadable_cursor_never_cancels() {
    let (monitor, desktop, cancel) = monitor();
    desktop.set_cursor(None);
    assert!((0..5).all(|_| monitor.checkpoint()));
    assert!(!cancel.is_cancelled());
}

#[test]
fn a_cancelled_run_fails_every_checkpoint() {
    let (monitor, _, cancel) = monitor();
    cancel.cancel();
    assert!(!monitor.checkpoint());
    assert_eq!(monitor.reason(), None);
}

#[test]
fn the_focus_watcher_cancels_once_the_game_stays_unfocused() {
    let (monitor, desktop, cancel) = monitor();
    desktop.set_focused(false);
    let started = Instant::now();
    monitor.start();
    assert!(wait_until(Duration::from_secs(3), || cancel.is_cancelled()));
    // Not before the grace period has passed.
    assert!(started.elapsed() >= Duration::from_secs_f64(FOCUS_LOSS_GRACE_S));
    assert_eq!(monitor.reason().as_deref(), Some("game_window_unfocused"));
    monitor.stop();
}

#[test]
fn a_focused_game_is_never_cancelled_and_stop_ends_the_watcher() {
    let (monitor, desktop, cancel) = monitor();
    monitor.start();
    assert!(wait_until(Duration::from_secs(2), || desktop.focus_polls() >= 2));
    monitor.stop();
    let polls = desktop.focus_polls();
    std::thread::sleep(Duration::from_millis(450));
    assert_eq!(desktop.focus_polls(), polls);
    assert!(!cancel.is_cancelled());
}

/// An unfocused game whose second focus check hangs for `hang` (a Windows call that doesn't return).
struct HangingDesktop {
    polls: AtomicUsize,
    hang: Duration,
}

impl Desktop for HangingDesktop {
    fn cursor_position(&self) -> Option<(i32, i32)> {
        Some((0, 0))
    }

    fn game_has_focus(&self) -> bool {
        if self.polls.fetch_add(1, Ordering::SeqCst) == 1 {
            std::thread::sleep(self.hang);
        }
        false
    }
}

#[test]
fn stop_gives_up_on_a_stuck_watcher_which_then_never_trips() {
    let desktop = Arc::new(HangingDesktop { polls: AtomicUsize::new(0), hang: Duration::from_millis(1800) });
    let cancel = CancelToken::new();
    let monitor = SafetyMonitor::new(cancel.clone(), desktop.clone());
    monitor.start();
    // Wait until the watcher is inside the hanging check.
    assert!(wait_until(Duration::from_secs(2), || desktop.polls.load(Ordering::SeqCst) >= 2));
    let stopping = Instant::now();
    monitor.stop();
    let waited = stopping.elapsed();
    assert!(waited >= Duration::from_millis(900) && waited < Duration::from_millis(1500), "stop waited {waited:?}");
    // The stuck check returns after the grace period has long passed, but its watcher was stopped.
    std::thread::sleep(Duration::from_millis(1200));
    assert!(!cancel.is_cancelled());
    assert_eq!(monitor.reason(), None);
}

#[test]
fn start_clears_the_last_reason_and_baseline() {
    let (monitor, desktop, _) = monitor();
    checkpoints(&monitor, &desktop, &[(0, 0), (200, 0), (400, 0), (600, 0)]);
    assert!(monitor.reason().is_some());
    monitor.start();
    assert_eq!(monitor.reason(), None);
    monitor.stop();
}
