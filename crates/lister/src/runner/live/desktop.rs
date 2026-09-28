//! The real game window, the desktop the safety monitor watches, and the pauses between steps.

use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use input::{WindowRect, WindowsInput};

use super::{GameWindow, DEFAULT_STEP_DELAY_S, MAX_STEP_DELAY_S, MIN_STEP_DELAY_S, STEP_JITTER_S};
use crate::job::CancelToken;
use crate::runner::Desktop;

pub const WINDOW_NOT_FOUND: &str = "Dark and Darker window not found.";
pub const NOT_IN_FRONT: &str = "Couldn't bring Dark and Darker to the front — click the game window and try again.";
pub const NO_WINDOW_AREA: &str =
    "Couldn't read the size of the Dark and Darker window — make sure it isn't minimized and try again.";
/// Exclusive fullscreen needs a moment to come back after the window is activated.
const FULLSCREEN_SETTLE: Duration = Duration::from_secs(1);
/// `FullscreenMode` of exclusive fullscreen in the game's `GameUserSettings.ini`.
const EXCLUSIVE_FULLSCREEN: u32 = 0;
/// How often a cancellable sleep checks the token.
const SLEEP_POLL: Duration = Duration::from_millis(10);
/// The game must hold focus this long before a run clicks anything: exclusive fullscreen can
/// flicker focus for a moment after it is activated, which would otherwise stop the run at once.
const STEADY_FOCUS: Duration = Duration::from_millis(600);
/// How long to wait for steady focus before giving up.
const FOCUS_WAIT: Duration = Duration::from_secs(5);
/// While waiting, the window is brought forward again this often.
const FOCUS_RETRY: Duration = Duration::from_millis(1500);
/// How often focus is checked while waiting.
const FOCUS_POLL: Duration = Duration::from_millis(50);

/// Waits until `focused()` has been true for [`STEADY_FOCUS`] without a break, calling `retry()`
/// every [`FOCUS_RETRY`] while it isn't; `false` after [`FOCUS_WAIT`] or once cancelled. `now` and
/// `sleep` are the clock, so the logic is testable.
pub fn wait_for_steady_focus(
    focused: &mut dyn FnMut() -> bool,
    retry: &mut dyn FnMut(),
    now: &dyn Fn() -> Duration,
    sleep: &mut dyn FnMut(Duration),
    cancelled: &dyn Fn() -> bool,
) -> bool {
    let start = now();
    let mut focused_since: Option<Duration> = None;
    let mut last_retry = start;
    while !cancelled() {
        let t = now();
        if focused() {
            let since = *focused_since.get_or_insert(t);
            if t - since >= STEADY_FOCUS {
                return true;
            }
        } else {
            focused_since = None;
            if t - last_retry >= FOCUS_RETRY {
                retry();
                last_retry = t;
            }
        }
        if t - start >= FOCUS_WAIT {
            return false;
        }
        sleep(FOCUS_POLL);
    }
    false
}

/// The game's real window.
#[derive(Debug, Clone)]
pub struct LiveGameWindow {
    game_exe: String,
}

impl LiveGameWindow {
    pub fn new(game_exe: &str) -> Self {
        LiveGameWindow { game_exe: game_exe.to_string() }
    }
}

impl GameWindow for LiveGameWindow {
    fn bring_forward(&self, cancel: &CancelToken) -> Result<WindowRect, String> {
        let window = input::find_game_window().ok_or_else(|| WINDOW_NOT_FOUND.to_string())?;
        let mut sink = WindowsInput;
        // Taps Alt first (lifting Windows' foreground lock for a background app). A failure here is
        // not final: the focus check below decides, as DnDTools ignored activate()'s errors.
        let _ = input::bring_to_front(&mut sink, window);
        if window_mode() == Some(EXCLUSIVE_FULLSCREEN) {
            sleep_unless_cancelled(cancel, FULLSCREEN_SETTLE);
        }
        input::tap_alt(&mut sink, input::DEFAULT_TAP_ALT_DELAY);
        input::release_modifiers(&mut sink);
        let started = Instant::now();
        let steady = wait_for_steady_focus(
            &mut || screen::game_has_focus(&self.game_exe),
            &mut || {
                let _ = input::bring_to_front(&mut WindowsInput, window);
                input::release_modifiers(&mut WindowsInput);
            },
            &|| started.elapsed(),
            &mut |pause| sleep_unless_cancelled(cancel, pause),
            &|| cancel.is_cancelled(),
        );
        if !steady {
            return Err(NOT_IN_FRONT.to_string());
        }
        input::client_area(window)
            .filter(|area| area.width > 0 && area.height > 0)
            .ok_or_else(|| NO_WINDOW_AREA.to_string())
    }
}

/// The game's window mode from its settings file, if it can be read.
fn window_mode() -> Option<u32> {
    let content = std::fs::read_to_string(input::game_config_path()?).ok()?;
    input::window_mode_from_ini(&content)
}

/// The real cursor and window focus.
#[derive(Debug, Clone)]
pub struct LiveDesktop {
    game_exe: String,
}

impl LiveDesktop {
    pub fn new(game_exe: &str) -> Self {
        LiveDesktop { game_exe: game_exe.to_string() }
    }
}

impl Desktop for LiveDesktop {
    fn cursor_position(&self) -> Option<(i32, i32)> {
        screen::cursor_position()
    }

    fn game_has_focus(&self) -> bool {
        screen::game_has_focus(&self.game_exe)
    }
}

/// Sleeps `duration`, returning early once `cancel` is cancelled.
pub fn sleep_unless_cancelled(cancel: &CancelToken, duration: Duration) {
    let Some(end) = Instant::now().checked_add(duration) else {
        return;
    };
    while !cancel.is_cancelled() {
        let now = Instant::now();
        if now >= end {
            return;
        }
        std::thread::sleep((end - now).min(SLEEP_POLL));
    }
}

/// How long to pause after a step (DnDTools' `make_pause`): `sortSpeed` seconds, at least
/// [`MIN_STEP_DELAY_S`], plus up to [`STEP_JITTER_S`] at random.
pub fn step_pause(step_delay_s: f64) -> Duration {
    let setting = if step_delay_s.is_finite() && step_delay_s >= 0.0 { step_delay_s } else { DEFAULT_STEP_DELAY_S };
    let seconds = setting.clamp(MIN_STEP_DELAY_S, MAX_STEP_DELAY_S) + unit_jitter() * STEP_JITTER_S;
    Duration::from_secs_f64(seconds)
}

/// A value in `[0, 1)` that differs call to call: pacing jitter, not security (so no RNG crate).
fn unit_jitter() -> f64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos()));
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod steady_focus_tests {
    use super::*;
    use std::cell::Cell;

    /// Runs `wait_for_steady_focus` on a fake clock with `focus_at(t)` saying whether the game has
    /// focus at time `t`; returns the outcome, the time it took and how many retries it made.
    fn run(focus_at: impl Fn(Duration) -> bool) -> (bool, Duration, u32) {
        let clock = Cell::new(Duration::ZERO);
        let retries = Cell::new(0);
        let steady = wait_for_steady_focus(
            &mut || focus_at(clock.get()),
            &mut || retries.set(retries.get() + 1),
            &|| clock.get(),
            &mut |pause| clock.set(clock.get() + pause),
            &|| false,
        );
        (steady, clock.get(), retries.get())
    }

    #[test]
    fn steady_focus_starts_the_run_after_the_settle_time() {
        let (steady, took, retries) = run(|_| true);
        assert!(steady);
        assert!(took >= STEADY_FOCUS && took < STEADY_FOCUS + FOCUS_POLL * 2, "{took:?}");
        assert_eq!(retries, 0);
    }

    #[test]
    fn a_flicker_restarts_the_count() {
        // Focused, then a 100 ms flicker at 300 ms, then focused for good.
        let flicker = |t: Duration| !(Duration::from_millis(300)..Duration::from_millis(400)).contains(&t);
        let (steady, took, _) = run(flicker);
        assert!(steady);
        assert!(took >= Duration::from_millis(400) + STEADY_FOCUS, "{took:?}");
    }

    #[test]
    fn a_game_that_never_takes_focus_gives_up_after_retrying() {
        let (steady, took, retries) = run(|_| false);
        assert!(!steady);
        assert!(took >= FOCUS_WAIT, "{took:?}");
        assert!(retries >= 2, "retried {retries} times");
    }
}
