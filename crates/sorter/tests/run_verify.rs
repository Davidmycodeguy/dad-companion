//! Before/after pixel comparison: when a drag counts as having moved its item.

use screen::Frame;
use sorter::run::{compare_probes, mean_abs_diff, CHANGE_THRESHOLD};

#[test]
fn identical_frames_do_not_differ() {
    let frame = Frame::filled(4, 4, [40, 50, 60]);
    assert_eq!(mean_abs_diff(&frame, &frame), Some(0.0));
}

#[test]
fn the_difference_is_averaged_over_every_channel() {
    let dark = Frame::filled(2, 2, [0, 0, 0]);
    let red = Frame::filled(2, 2, [30, 0, 0]);
    assert_eq!(mean_abs_diff(&dark, &red), Some(10.0));
    assert_eq!(mean_abs_diff(&red, &dark), Some(10.0));
}

#[test]
fn frames_that_cannot_be_compared_never_verify_a_move() {
    let small = Frame::filled(2, 2, [0, 0, 0]);
    let large = Frame::filled(3, 3, [200, 200, 200]);
    let empty = Frame::new(0, 0, Vec::new());
    assert_eq!(mean_abs_diff(&small, &large), None);
    assert_eq!(mean_abs_diff(&empty, &empty), None);
    assert!(!compare_probes((&small, &small), (&large, &large)).verified());
}

#[test]
fn a_move_is_verified_when_its_source_cleared_or_its_destination_filled() {
    let empty = Frame::filled(4, 4, [22, 20, 18]);
    let item = Frame::filled(4, 4, [180, 90, 60]);

    let both = compare_probes((&item, &empty), (&empty, &item));
    assert!(both.source_changed() && both.dest_changed() && both.verified());

    let only_source = compare_probes((&item, &item), (&empty, &item));
    assert!(only_source.source_changed() && !only_source.dest_changed() && only_source.verified());

    let only_dest = compare_probes((&item, &empty), (&item, &item));
    assert!(only_dest.verified());

    let neither = compare_probes((&item, &empty), (&item, &empty));
    assert!(!neither.verified());
}

#[test]
fn a_change_at_the_threshold_is_not_enough() {
    let before = Frame::filled(1, 1, [0, 0, 0]);
    let at = Frame::filled(1, 1, [8, 8, 8]);
    let above = Frame::filled(1, 1, [9, 9, 9]);
    assert_eq!(CHANGE_THRESHOLD, 8.0);
    assert!(!compare_probes((&before, &before), (&at, &at)).verified());
    assert!(compare_probes((&before, &before), (&above, &above)).verified());
}
