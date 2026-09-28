//! Pure layout-scaling tests at the resolutions the task calls out explicitly: 1920x1080,
//! 2560x1440, 3840x2160, and an ultrawide.

use input::{is_ultrawide, positions_for_resolution};

#[test]
fn at_1920x1080_positions_match_the_base_layout_exactly() {
    let layout = positions_for_resolution((1920, 1080));
    assert_eq!(layout.stash, (1378.0, 199.0));
    assert_eq!(layout.inv, (690.0, 626.0));
    assert_eq!(layout.jump, 40.5);
    assert_eq!(layout.stash_tab_origin, (1328.0, 211.0));
    assert_eq!(layout.stash_tab_spacing, 45.0);
    assert!(!is_ultrawide((1920, 1080)));
}

#[test]
fn at_2560x1440_everything_scales_by_four_thirds() {
    // 2560/1920 == 1440/1080 == 4/3 exactly, and 2560x1440 is exactly 16:9, so this is the plain
    // (non-ultrawide) scaling branch.
    assert!(!is_ultrawide((2560, 1440)));
    let layout = positions_for_resolution((2560, 1440));
    assert_eq!(layout.stash, (1837.0, 265.0)); // round(1378*4/3)=round(1837.33..)=1837, round(199*4/3)=round(265.33..)=265
    assert_eq!(layout.jump, 54.0); // 40.5 * 4/3
    assert_eq!(layout.stash_tab_spacing, 60.0); // 45 * 4/3
}

#[test]
fn at_3840x2160_everything_scales_by_two() {
    assert!(!is_ultrawide((3840, 2160)));
    let layout = positions_for_resolution((3840, 2160));
    assert_eq!(layout.stash, (2756.0, 398.0));
    assert_eq!(layout.inv, (1380.0, 1252.0));
    assert_eq!(layout.jump, 81.0);
    assert_eq!(layout.stash_tab_origin, (2656.0, 422.0));
    assert_eq!(layout.stash_tab_spacing, 90.0);
}

#[test]
fn at_ultrawide_3440x1440_the_layout_is_scaled_by_height_and_letterboxed() {
    assert!(is_ultrawide((3440, 1440)));
    let layout = positions_for_resolution((3440, 1440));
    // scale = 1440/1080 (height ratio, both axes); offset_x = (3440 - 1440*16/9)/2 = 440.
    // stash: 1378 * 4/3 + 440 = 1837.33.. + 440 = 2277.33.. -> 2277; 199 * 4/3 = 265.33.. -> 265.
    assert_eq!(layout.stash, (2277.0, 265.0));
    // jump is a length, not a point: no horizontal offset applies, just the 4/3 scale.
    assert_eq!(layout.jump, 54.0);
}

#[test]
fn super_ultrawide_5120x1440_is_also_classified_as_ultrawide() {
    assert!(is_ultrawide((5120, 1440)));
    let layout = positions_for_resolution((5120, 1440));
    assert_eq!(layout.jump, 54.0); // same 4/3 height-based scale as any other 1440-tall ultrawide
}
