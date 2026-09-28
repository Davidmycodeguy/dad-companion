//! `grab` tests: a real, tiny screen capture, plus the empty/inverted-region fast path that
//! never touches the screen (mirrors `screen_grab.py`'s behaviour for a degenerate region).

use screen::{grab, make_process_dpi_aware};

#[test]
fn small_top_left_region_has_the_requested_size() {
    // Physical pixels matter here (see the crate docs); best-effort, as tests have no manifest.
    let _ = make_process_dpi_aware();

    let frame = grab((0, 0, 64, 32)).expect("grabbing a small on-screen region should succeed");
    assert_eq!((frame.width(), frame.height()), (64, 32));
}

#[test]
fn empty_region_returns_an_empty_frame() {
    let frame = grab((100, 100, 100, 100)).expect("an empty region should not touch the screen");
    assert_eq!((frame.width(), frame.height()), (0, 0));
}

#[test]
fn inverted_region_returns_an_empty_frame() {
    let frame = grab((100, 100, 50, 50)).expect("an inverted region should not touch the screen");
    assert_eq!((frame.width(), frame.height()), (0, 0));
}
