//! Stopping a run the moment driving the mouse is no longer safe, and never leaving a button held.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use input::{Cancel, InputEvent, InputSink, VK_MENU};
use screen::{Frame, Region};
use sorter::run::{spawn_watchdog, GuardedSink, InterferenceGuard, Machine, Safety, StopReason};

const SETTLE: Duration = Duration::from_millis(20);

#[test]
fn the_cursor_where_the_sorter_left_it_is_fine_and_drift_is_the_player() {
    let start = Instant::now();
    let mut guard = InterferenceGuard::new(16, SETTLE);
    guard.commanded((500, 300), start);
    let later = start + Duration::from_millis(50);
    assert!(guard.observe((500, 300), later));
    assert!(guard.observe((516, 284), later), "within 16 px is still the sorter's cursor");
    assert!(!guard.observe((517, 300), later));
    assert!(!guard.observe((500, 250), later));
}

#[test]
fn right_after_its_own_move_the_sorter_lets_the_cursor_arrive() {
    let start = Instant::now();
    let mut guard = InterferenceGuard::new(16, SETTLE);
    guard.commanded((500, 300), start);
    assert!(guard.observe((100, 100), start + Duration::from_millis(5)));
    assert!(!guard.observe((100, 100), start + SETTLE));
}

#[test]
fn a_baseline_is_checked_straight_away() {
    let mut guard = InterferenceGuard::new(16, SETTLE);
    assert!(guard.observe((9999, 9999), Instant::now()), "nothing to compare with yet");
    guard.baseline((10, 10));
    assert!(!guard.observe((60, 10), Instant::now()));
}

#[test]
fn the_tolerance_grows_with_the_cells() {
    assert_eq!(InterferenceGuard::tolerance_for(10.0), 12);
    assert_eq!(InterferenceGuard::tolerance_for(40.5), 16);
    assert_eq!(InterferenceGuard::tolerance_for(81.0), 32);
}

#[test]
fn nothing_is_checked_before_the_safety_is_armed() {
    let safety = Safety::new(InterferenceGuard::new(16, Duration::ZERO));
    let now = Instant::now();
    assert_eq!(safety.poll(now, || (false, Some((0, 0)))), None);
    assert_eq!(safety.poll(now + Duration::from_secs(5), || (false, Some((900, 900)))), None);
}

#[test]
fn a_short_focus_loss_is_forgiven_and_a_long_one_stops_the_run() {
    let safety = Safety::new(InterferenceGuard::new(16, Duration::ZERO));
    safety.arm(Some((10, 10)));
    let start = Instant::now();
    assert_eq!(safety.poll(start, || (false, Some((10, 10)))), None);
    assert_eq!(safety.poll(start + Duration::from_millis(300), || (true, Some((10, 10)))), None);
    assert_eq!(safety.poll(start + Duration::from_millis(400), || (false, Some((10, 10)))), None);
    assert_eq!(safety.poll(start + Duration::from_millis(1100), || (false, Some((10, 10)))), Some(StopReason::FocusLost));
    assert_eq!(safety.tripped(), Some(StopReason::FocusLost));
}

#[test]
fn moving_the_mouse_after_arming_stops_the_run_and_the_first_reason_wins() {
    let safety = Safety::new(InterferenceGuard::new(16, Duration::ZERO));
    safety.arm(Some((10, 10)));
    assert_eq!(safety.poll(Instant::now(), || (true, Some((200, 10)))), Some(StopReason::MouseMoved));
    safety.trip(StopReason::Cancelled);
    assert_eq!(safety.tripped(), Some(StopReason::MouseMoved));
}

#[test]
fn the_sorters_own_moves_become_the_new_baseline() {
    let safety = Safety::new(InterferenceGuard::new(16, Duration::ZERO));
    safety.arm(Some((10, 10)));
    safety.commanded_move((400, 400), || {});
    assert_eq!(safety.poll(Instant::now(), || (true, Some((400, 400)))), None);
    assert_eq!(safety.poll(Instant::now(), || (true, Some((10, 10)))), Some(StopReason::MouseMoved));
}

#[derive(Default)]
struct Recording(Vec<InputEvent>);

impl InputSink for Recording {
    fn move_to(&mut self, x: i32, y: i32) {
        self.0.push(InputEvent::MoveTo { x, y });
    }
    fn mouse_down(&mut self) {
        self.0.push(InputEvent::MouseDown);
    }
    fn mouse_up(&mut self) {
        self.0.push(InputEvent::MouseUp);
    }
    fn key_event(&mut self, vk: u16, key_up: bool) {
        self.0.push(InputEvent::Key { vk, key_up });
    }
    fn cursor_position(&self) -> (i32, i32) {
        (0, 0)
    }
}

#[test]
fn a_cancelled_sink_still_releases_but_never_presses_or_moves() {
    let safety = Safety::new(InterferenceGuard::new(16, Duration::ZERO));
    let cancel = Cancel::new();
    let mut inner = Recording::default();
    {
        let mut sink = GuardedSink::new(&mut inner, &safety, &cancel);
        sink.move_to(5, 5);
        sink.mouse_down();
        cancel.cancel();
        sink.move_to(50, 50);
        sink.mouse_down();
        sink.key_event(VK_MENU, false);
        sink.mouse_up();
        sink.key_event(VK_MENU, true);
    }
    let expected = vec![
        InputEvent::MoveTo { x: 5, y: 5 },
        InputEvent::MouseDown,
        InputEvent::MouseUp,
        InputEvent::Key { vk: VK_MENU, key_up: true },
    ];
    assert_eq!(inner.0, expected);
}

/// A machine whose cursor the test controls; always focused.
struct Pointer(Mutex<(i32, i32)>);

impl Machine for Pointer {
    fn grab(&self, (l, t, r, b): Region) -> Result<Frame, String> {
        Ok(Frame::filled((r - l) as usize, (b - t) as usize, [0, 0, 0]))
    }
    fn cursor(&self) -> Option<(i32, i32)> {
        Some(*self.0.lock().unwrap())
    }
    fn game_focused(&self) -> bool {
        true
    }
    fn bring_game_forward(&self, _sink: &mut dyn InputSink) -> Result<(), StopReason> {
        Ok(())
    }
}

#[test]
fn the_watchdog_cancels_the_run_as_soon_as_a_check_fails() {
    let safety = Arc::new(Safety::new(InterferenceGuard::new(16, Duration::ZERO)));
    safety.arm(Some((10, 10)));
    let pointer = Arc::new(Pointer(Mutex::new((10, 10))));
    let cancel = Cancel::new();
    let watchdog = spawn_watchdog(Arc::clone(&safety), pointer.clone(), cancel.clone(), Duration::from_millis(2)).expect("thread starts");

    std::thread::sleep(Duration::from_millis(30));
    assert!(!cancel.is_cancelled(), "a still cursor is fine");
    *pointer.0.lock().unwrap() = (300, 10);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !cancel.is_cancelled() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(cancel.is_cancelled());
    assert_eq!(safety.tripped(), Some(StopReason::MouseMoved));
    watchdog.stop();
}
