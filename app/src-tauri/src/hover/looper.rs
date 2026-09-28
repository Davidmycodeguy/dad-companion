//! Watches the cursor while the game is in front; when it rests on an item, shows that item's value.
//! Port of DnDTools' `hover_loop.py`.
//!
//! The game draws an item's tooltip ~0.2-0.3 s after the cursor comes to rest. Rather than wait a
//! fixed time, the loop looks for it on every poll from the moment the cursor settles (a look grabs
//! only two thin strips beside the cursor, so an empty look is cheap), reads it the moment it
//! appears, and hides the card as soon as the cursor moves on or the game's tooltip goes away.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::overlay::Region;

/// Cursor checks; a resting cursor's tooltip is looked for this often.
pub const POLL_S: f64 = 0.02;
/// The cursor has stopped, not just passed over an item.
pub const SETTLE_S: f64 = 0.05;
/// Look on every poll this long (tooltips appear ~0.2-0.3 s in) ...
pub const EAGER_S: f64 = 0.6;
/// ... then less often ...
pub const LOOK_EVERY_S: f64 = 0.1;
/// ... and stop: nothing is coming at this spot.
pub const LOOK_FOR_S: f64 = 1.5;
/// While the card shows: is the game's tooltip still up?
pub const CHECK_EVERY_S: f64 = 0.25;
/// Cursor movement that counts as moving on.
pub const MOVE_PX: f64 = 6.0;
/// Screen height the finder's pixel constants are measured at (4K).
const BASE_HEIGHT: f64 = 2160.0;
/// After a failed step, wait this long so a lasting fault doesn't flood the log.
const ERROR_BACKOFF: Duration = Duration::from_secs(5);

/// What the loop needs to know about the world.
pub trait Senses {
    /// Hover values are switched on.
    fn enabled(&self) -> bool;
    /// The game is the focused window.
    fn game_active(&self) -> bool;
    fn cursor(&self) -> (i32, i32);
    /// The game's screen in physical pixels.
    fn screen_size(&self) -> (i32, i32);
    /// A monotonic clock in seconds.
    fn now(&self) -> f64;
}

/// Finds the tooltip beside the cursor and checks it is still up.
pub trait Reader {
    /// A tooltip read from the screen (its parsed text and where it is).
    type Found;
    fn read_screen(&mut self, cursor: (i32, i32), scale: f64, screen: (i32, i32)) -> Result<Option<Self::Found>, String>;
    fn still_showing(&mut self, cursor: (i32, i32), scale: f64, screen: (i32, i32), found: &Self::Found) -> Result<bool, String>;
    /// The tooltip's title bar down to its rule, in screen pixels: where the card goes beside.
    fn tooltip_region(&self, found: &Self::Found, scale: f64) -> Region;
}

/// Shows and hides the card.
pub trait Panel<F> {
    /// Shows the card for `found` beside `tooltip`; false when there is no card for it.
    fn show(&mut self, found: &F, tooltip: Region, screen: (i32, i32)) -> bool;
    fn hide(&mut self);
}

pub struct HoverLoop<S, R: Reader, P> {
    senses: S,
    reader: R,
    panel: P,
    /// Last cursor position.
    last: Option<(i32, i32)>,
    /// When the cursor settled there.
    since: f64,
    /// Last look (or check) at this rest.
    looked: f64,
    /// Nothing more to look for until the cursor moves.
    done: bool,
    /// The tooltip the card is showing for.
    shown: Option<R::Found>,
    /// Last on/off state, logged when it changes.
    active: Option<bool>,
}

impl<S: Senses, R: Reader, P: Panel<R::Found>> HoverLoop<S, R, P> {
    pub fn new(senses: S, reader: R, panel: P) -> Self {
        Self { senses, reader, panel, last: None, since: 0.0, looked: f64::NEG_INFINITY, done: false, shown: None, active: None }
    }

    fn hide(&mut self) {
        if self.shown.take().is_some() {
            self.panel.hide();
        }
    }

    fn geometry(&self) -> ((i32, i32), f64) {
        let screen = self.senses.screen_size();
        (screen, f64::from(screen.1) / BASE_HEIGHT)
    }

    /// One poll: look for, show, check or hide the card.
    pub fn step(&mut self) {
        let active = self.senses.enabled() && self.senses.game_active();
        if self.active != Some(active) {
            log::info!("hover values {}", if active { "watching the cursor" } else { "paused" });
            self.active = Some(active);
        }
        if !active {
            self.hide();
            self.last = None;
            return;
        }
        let (position, now) = (self.senses.cursor(), self.senses.now());
        let moved = self.last.is_none_or(|last| distance(position, last) > MOVE_PX);
        if moved {
            (self.last, self.since, self.looked, self.done) = (Some(position), now, f64::NEG_INFINITY, false);
            self.hide();
            return;
        }
        if self.shown.is_some() {
            if now - self.looked >= CHECK_EVERY_S {
                self.looked = now;
                self.check(position);
            }
            return;
        }
        let rest = now - self.since;
        if self.done || rest < SETTLE_S {
            return;
        }
        if rest > LOOK_FOR_S {
            self.done = true;
            return;
        }
        if rest > EAGER_S && now - self.looked < LOOK_EVERY_S {
            return;
        }
        self.looked = now;
        self.look(position);
    }

    fn look(&mut self, position: (i32, i32)) {
        let (screen, scale) = self.geometry();
        let found = match self.reader.read_screen(position, scale, screen) {
            Ok(Some(found)) => found,
            Ok(None) => return,
            Err(err) => {
                // A bad read must never stop the loop, nor repeat on every poll.
                log::warn!("hover value read failed: {err}");
                self.done = true;
                return;
            }
        };
        let tooltip = self.reader.tooltip_region(&found, scale);
        let read_at = self.senses.now();
        if self.panel.show(&found, tooltip, screen) {
            self.shown = Some(found);
            self.done = true;
            log::debug!(
                "hover card sent {:.0} ms after the cursor rested (read took {:.0} ms)",
                1000.0 * (self.senses.now() - self.since),
                1000.0 * (read_at - self.looked),
            );
        }
    }

    fn check(&mut self, position: (i32, i32)) {
        let (screen, scale) = self.geometry();
        let Some(found) = self.shown.as_ref() else { return };
        // Keep the card rather than flicker it on a bad capture.
        let showing = self.reader.still_showing(position, scale, screen, found).unwrap_or(true);
        if !showing {
            self.hide();
        }
    }

    /// Polls until `stop` is set, then hides the card.
    pub fn run(&mut self, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.step()));
            match step {
                Ok(()) => std::thread::sleep(Duration::from_secs_f64(POLL_S)),
                Err(_) => {
                    log::error!("hover value loop step failed");
                    std::thread::sleep(ERROR_BACKOFF);
                }
            }
        }
        self.hide();
    }
}

fn distance(a: (i32, i32), b: (i32, i32)) -> f64 {
    f64::from(a.0 - b.0).hypot(f64::from(a.1 - b.1))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    const SCREEN: (i32, i32) = (3840, 2160);
    /// The finder's title height at 4K.
    const TITLE_HEIGHT_PX: f64 = 110.0;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Found {
        left: i32,
        right: i32,
        rule_y: i32,
    }

    const BOX: Found = Found { left: 2030, right: 2700, rule_y: 700 };

    #[derive(Default)]
    struct World {
        clock: Cell<f64>,
        cursor: Cell<(i32, i32)>,
        enabled: Cell<bool>,
        active: Cell<bool>,
    }

    struct FakeSenses(Rc<World>);

    impl Senses for FakeSenses {
        fn enabled(&self) -> bool {
            self.0.enabled.get()
        }
        fn game_active(&self) -> bool {
            self.0.active.get()
        }
        fn cursor(&self) -> (i32, i32) {
            self.0.cursor.get()
        }
        fn screen_size(&self) -> (i32, i32) {
            SCREEN
        }
        fn now(&self) -> f64 {
            self.0.clock.get()
        }
    }

    /// One look: where the cursor was, the scale, and the screen size.
    type Look = ((i32, i32), f64, (i32, i32));

    /// Finds BOX once `appears_after` looks have come up empty; the tooltip stays up while `up`.
    #[derive(Default)]
    struct Log {
        looks: RefCell<Vec<Look>>,
        checks: Cell<u32>,
        up: Cell<bool>,
        shown: RefCell<Vec<Region>>,
        hidden: Cell<u32>,
    }

    struct FakeReader {
        log: Rc<Log>,
        appears_after: usize,
        fail: bool,
    }

    impl Reader for FakeReader {
        type Found = Found;
        fn read_screen(&mut self, cursor: (i32, i32), scale: f64, screen: (i32, i32)) -> Result<Option<Found>, String> {
            self.log.looks.borrow_mut().push((cursor, scale, screen));
            if self.fail {
                return Err("OCR worker died".into());
            }
            Ok((self.log.looks.borrow().len() > self.appears_after).then_some(BOX))
        }
        fn still_showing(&mut self, _: (i32, i32), _: f64, _: (i32, i32), _: &Found) -> Result<bool, String> {
            self.log.checks.set(self.log.checks.get() + 1);
            Ok(self.log.up.get())
        }
        fn tooltip_region(&self, found: &Found, scale: f64) -> Region {
            (found.left, found.rule_y - (TITLE_HEIGHT_PX * scale).round() as i32, found.right, found.rule_y)
        }
    }

    struct FakePanel(Rc<Log>);

    impl Panel<Found> for FakePanel {
        fn show(&mut self, _: &Found, tooltip: Region, _: (i32, i32)) -> bool {
            self.0.shown.borrow_mut().push(tooltip);
            true
        }
        fn hide(&mut self) {
            self.0.hidden.set(self.0.hidden.get() + 1);
        }
    }

    type Loop = HoverLoop<FakeSenses, FakeReader, FakePanel>;

    fn setup(appears_after: usize, fail: bool, enabled: bool, active: bool) -> (Loop, Rc<World>, Rc<Log>) {
        let world = Rc::new(World::default());
        world.cursor.set((2000, 1000));
        world.enabled.set(enabled);
        world.active.set(active);
        let log = Rc::new(Log::default());
        log.up.set(true);
        let reader = FakeReader { log: log.clone(), appears_after, fail };
        (HoverLoop::new(FakeSenses(world.clone()), reader, FakePanel(log.clone())), world, log)
    }

    fn run(hover: &mut Loop, world: &World, seconds: f64) {
        let end = world.clock.get() + seconds;
        while world.clock.get() < end - 1e-9 {
            world.clock.set(((world.clock.get() + POLL_S) * 1e6).round() / 1e6);
            hover.step();
        }
    }

    #[test]
    fn looks_as_soon_as_the_cursor_settles_and_shows_the_card_beside_the_tooltip() {
        let (mut hover, world, log) = setup(0, false, true, true);
        hover.step();
        assert!(log.looks.borrow().is_empty()); // just arrived: could be passing over the item
        run(&mut hover, &world, SETTLE_S + 0.02);
        assert_eq!(*log.looks.borrow(), [((2000, 1000), 1.0, SCREEN)]);
        assert_eq!(*log.shown.borrow(), [(2030, 700 - 110, 2700, 700)]); // title bar above the rule
        run(&mut hover, &world, 0.2);
        assert_eq!(log.looks.borrow().len(), 1); // found: no more looks while resting on it
    }

    #[test]
    fn a_tooltip_the_game_draws_late_is_caught_on_the_next_poll() {
        let (mut hover, world, log) = setup(10, false, true, true); // ~0.2 s of empty looks
        hover.step();
        run(&mut hover, &world, SETTLE_S + 0.3);
        assert_eq!((log.looks.borrow().len(), log.shown.borrow().len()), (11, 1));
    }

    #[test]
    fn an_empty_spot_is_looked_at_less_after_a_while_then_not_at_all() {
        let (mut hover, world, log) = setup(10_000, false, true, true);
        hover.step();
        run(&mut hover, &world, EAGER_S);
        let eager = log.looks.borrow().len();
        run(&mut hover, &world, LOOK_FOR_S - EAGER_S);
        let patient = log.looks.borrow().len() - eager;
        assert!(eager as f64 >= (EAGER_S - SETTLE_S) / POLL_S - 2.0, "{eager} eager looks");
        assert!(patient as f64 <= (LOOK_FOR_S - EAGER_S) / LOOK_EVERY_S + 1.0, "{patient} patient looks");
        let total = log.looks.borrow().len();
        run(&mut hover, &world, 2.0);
        assert_eq!(log.looks.borrow().len(), total);
        assert!(log.shown.borrow().is_empty());
    }

    #[test]
    fn moving_hides_the_card_and_looks_again_once_settled() {
        let (mut hover, world, log) = setup(0, false, true, true);
        hover.step();
        run(&mut hover, &world, SETTLE_S + 0.02);
        world.cursor.set((2000 + MOVE_PX as i32 + 5, 1000));
        run(&mut hover, &world, 0.02);
        assert_eq!(log.hidden.get(), 1);
        run(&mut hover, &world, SETTLE_S + 0.02);
        assert_eq!((log.looks.borrow().len(), log.shown.borrow().len()), (2, 2));
    }

    #[test]
    fn the_card_goes_when_the_games_tooltip_does() {
        let (mut hover, world, log) = setup(0, false, true, true);
        hover.step();
        run(&mut hover, &world, SETTLE_S + 0.02);
        run(&mut hover, &world, CHECK_EVERY_S * 2.0);
        assert_eq!(log.hidden.get(), 0);
        assert!((1..=3).contains(&log.checks.get()), "checked now and then, not every poll");
        log.up.set(false); // e.g. a key closed the tooltip
        run(&mut hover, &world, CHECK_EVERY_S + 0.02);
        assert_eq!(log.hidden.get(), 1);
        let looks = log.looks.borrow().len();
        run(&mut hover, &world, 1.0);
        assert_eq!(log.looks.borrow().len(), looks); // not re-read until the cursor moves
    }

    #[test]
    fn off_or_game_in_background_means_no_looks_and_hides_the_card() {
        for (enabled, active) in [(false, true), (true, false)] {
            let (mut hover, world, log) = setup(0, false, enabled, active);
            hover.step();
            run(&mut hover, &world, 0.5);
            assert!(log.looks.borrow().is_empty() && log.shown.borrow().is_empty());
        }
        let (mut hover, world, log) = setup(0, false, true, true);
        hover.step();
        run(&mut hover, &world, SETTLE_S + 0.02);
        world.enabled.set(false);
        run(&mut hover, &world, 0.1);
        assert_eq!(log.hidden.get(), 1);
    }

    #[test]
    fn a_failing_read_stops_looking_until_the_cursor_moves() {
        let (mut hover, world, log) = setup(0, true, true, true);
        hover.step();
        run(&mut hover, &world, 1.0); // must not panic, nor retry every poll
        assert_eq!(log.looks.borrow().len(), 1);
        assert!(log.shown.borrow().is_empty());
    }
}
