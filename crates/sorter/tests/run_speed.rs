//! Drag pacing: slower after an unconfirmed move, back toward the player's speed after good ones.

use std::time::Duration;

use sorter::run::AdaptiveSpeed;

const MS: fn(u64) -> Duration = Duration::from_millis;

#[test]
fn it_starts_at_the_players_delay() {
    let speed = AdaptiveSpeed::new(MS(200));
    assert_eq!((speed.delay(), speed.streak()), (MS(200), 0));
}

#[test]
fn an_unconfirmed_move_slows_down_by_half_and_resets_the_streak() {
    let mut speed = AdaptiveSpeed::new(MS(200));
    speed.record(true);
    speed.record(false);
    assert_eq!((speed.delay(), speed.streak()), (MS(300), 0));
}

#[test]
fn slowing_down_from_instant_starts_at_fifty_milliseconds() {
    let mut speed = AdaptiveSpeed::new(Duration::ZERO);
    speed.record(false);
    assert_eq!(speed.delay(), MS(75));
}

#[test]
fn it_never_gets_slower_than_half_a_second() {
    let mut speed = AdaptiveSpeed::new(MS(900));
    assert_eq!(speed.delay(), MS(500));
    for _ in 0..5 {
        speed.record(false);
    }
    assert_eq!(speed.delay(), MS(500));
}

#[test]
fn after_five_good_moves_in_a_row_it_eases_back_but_never_below_the_players_delay() {
    let mut speed = AdaptiveSpeed::new(MS(200));
    speed.record(false); // 300 ms
    for _ in 0..4 {
        speed.record(true);
    }
    assert_eq!(speed.delay(), MS(300), "four in a row is not enough");
    speed.record(true);
    assert_eq!(speed.delay(), MS(270));
    for _ in 0..20 {
        speed.record(true);
    }
    assert_eq!(speed.delay(), MS(200));
    assert_eq!(speed.streak(), 25);
}
