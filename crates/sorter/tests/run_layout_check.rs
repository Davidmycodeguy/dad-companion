//! The pre-run check that the stash on screen is the one the plan was built from.

use screen::Frame;
use sorter::run::{check_layout, CellPatch, LayoutVerdict};

const EMPTY: [u8; 3] = [22, 20, 18];

fn cell(in_stash: bool, expected_occupied: bool, rgb: [u8; 3]) -> CellPatch {
    CellPatch { in_stash, expected_occupied, pixels: Frame::filled(6, 6, rgb) }
}

fn item_colour(n: usize) -> [u8; 3] {
    [(80 + n * 37 % 150) as u8, (70 + n * 53 % 160) as u8, (60 + n * 17 % 170) as u8]
}

/// A 40-cell stash (first `occupied` cells hold items) and a 10-cell bag (first 2 hold items),
/// captured exactly as expected.
fn expected_screen(occupied: usize) -> Vec<CellPatch> {
    let stash = (0..40).map(|i| if i < occupied { cell(true, true, item_colour(i)) } else { cell(true, false, EMPTY) });
    let bag = (0..10).map(|i| if i < 2 { cell(false, true, item_colour(100 + i)) } else { cell(false, false, EMPTY) });
    stash.chain(bag).collect()
}

#[test]
fn the_expected_stash_on_screen_matches() {
    let check = check_layout(&expected_screen(15));
    assert_eq!(check.verdict, LayoutVerdict::Match);
    assert_eq!((check.empty_seen, check.empty_total), (33, 33));
    assert_eq!((check.occupied_seen, check.occupied_total), (17, 17));
}

#[test]
fn items_where_cells_should_be_empty_are_a_mismatch() {
    // Another tab: items fill cells the plan expects to be empty.
    let mut screen = expected_screen(15);
    for (i, patch) in screen.iter_mut().enumerate().take(30).skip(15) {
        patch.pixels = Frame::filled(6, 6, item_colour(i + 7));
    }
    assert_eq!(check_layout(&screen).verdict, LayoutVerdict::Mismatch);
}

#[test]
fn a_stash_emptier_than_expected_is_a_mismatch() {
    // The locked seasonal stash has far fewer items than the tab being sorted.
    let mut screen = expected_screen(30);
    for patch in screen.iter_mut().take(20) {
        patch.pixels = Frame::filled(6, 6, EMPTY);
    }
    assert_eq!(check_layout(&screen).verdict, LayoutVerdict::Mismatch);
}

#[test]
fn a_matching_bag_cannot_hide_a_wrong_stash_and_the_reverse() {
    let mut wrong_bag = expected_screen(15);
    for patch in wrong_bag.iter_mut().filter(|p| !p.in_stash) {
        patch.pixels = Frame::filled(6, 6, item_colour(3));
    }
    assert_eq!(check_layout(&wrong_bag).verdict, LayoutVerdict::Mismatch);
}

#[test]
fn a_full_stash_is_checked_against_the_bags_empty_cells() {
    let full = expected_screen(40);
    assert_eq!(check_layout(&full).verdict, LayoutVerdict::Match);

    let mut looks_empty = expected_screen(40);
    for patch in looks_empty.iter_mut().take(25) {
        patch.pixels = Frame::filled(6, 6, EMPTY);
    }
    assert_eq!(check_layout(&looks_empty).verdict, LayoutVerdict::Mismatch);
}

#[test]
fn with_too_few_empty_cells_the_screen_cannot_be_read() {
    let mut screen: Vec<CellPatch> = (0..20).map(|i| cell(true, true, item_colour(i))).collect();
    screen.push(cell(true, false, EMPTY));
    screen.push(cell(false, false, EMPTY));
    let check = check_layout(&screen);
    assert_eq!(check.verdict, LayoutVerdict::Unreadable);
    assert_eq!((check.empty_total, check.occupied_total), (2, 20));
}

#[test]
fn a_few_plain_corners_of_large_items_are_tolerated() {
    // 3 of 15 occupied stash cells (20%) look like empty background.
    let mut screen = expected_screen(15);
    for patch in screen.iter_mut().take(3) {
        patch.pixels = Frame::filled(6, 6, EMPTY);
    }
    assert_eq!(check_layout(&screen).verdict, LayoutVerdict::Match);
}

#[test]
fn slight_noise_on_empty_cells_still_matches() {
    let mut screen = expected_screen(15);
    for (i, patch) in screen.iter_mut().enumerate().filter(|(_, p)| !p.expected_occupied) {
        let d = (i % 5) as u8;
        patch.pixels = Frame::filled(6, 6, [EMPTY[0] + d, EMPTY[1] + d, EMPTY[2] + d]);
    }
    assert_eq!(check_layout(&screen).verdict, LayoutVerdict::Match);
}
