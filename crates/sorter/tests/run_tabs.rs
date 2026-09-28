//! Which stash tab selector the sorter may click (never the locked stash's), and where the grids
//! are on screen for a given window and calibration.

use input::positions_for_resolution;
use sorter::run::{layout_for, tab_index};
use state::stash::{BAG, EQUIPMENT, STORAGE};
use state::LOCKED_SEASONAL_STASH;

/// The stash tabs this account's character lists, in the game's order.
const STORAGES: [u32; 5] = [4, 20, 5, 21, 30];

#[test]
fn without_a_saved_mapping_tabs_follow_the_games_stash_order() {
    assert_eq!(tab_index(None, &STORAGES, STORAGE), Some(0));
    assert_eq!(tab_index(None, &STORAGES, 20), Some(1));
    assert_eq!(tab_index(None, &STORAGES, 5), Some(2));
    assert_eq!(tab_index(None, &STORAGES, 21), Some(3));
    assert_eq!(tab_index(Some(&[0; 8]), &STORAGES, 21), Some(3), "an all-zero mapping means none");
}

#[test]
fn a_saved_mapping_wins() {
    let mapping = [4, 20, 5, 6, 7, 8, 9, 30];
    assert_eq!(tab_index(Some(&mapping), &STORAGES, 5), Some(2));
    assert_eq!(tab_index(Some(&mapping), &STORAGES, 9), Some(6));
    assert_eq!(tab_index(Some(&mapping), &STORAGES, 21), None, "not in the player's mapping");
}

#[test]
fn the_locked_seasonal_stash_never_gets_a_tab() {
    assert_eq!(tab_index(None, &STORAGES, LOCKED_SEASONAL_STASH), None);
    assert_eq!(tab_index(Some(&[30, 4, 0, 0, 0, 0, 0, 0]), &STORAGES, LOCKED_SEASONAL_STASH), None);
}

#[test]
fn the_bag_equipment_and_unknown_stashes_have_no_tab() {
    let storages = [BAG, EQUIPMENT, 4, 5];
    assert_eq!(tab_index(None, &storages, BAG), None);
    assert_eq!(tab_index(None, &storages, EQUIPMENT), None);
    assert_eq!(tab_index(None, &storages, 6), None);
    assert_eq!(tab_index(None, &storages, 5), Some(1), "the bag and equipment don't take a selector");
}

#[test]
fn gear_sets_and_repeats_do_not_count_and_only_eight_selectors_exist() {
    assert_eq!(tab_index(None, &[4, 4, 101, 5], 5), Some(1));
    let many = [4, 5, 6, 7, 8, 9, 20, 21, 22];
    assert_eq!(tab_index(None, &many, 21), Some(7));
    assert_eq!(tab_index(None, &many, 22), None);
}

#[test]
fn a_window_the_size_of_the_game_moves_the_whole_layout() {
    let stock = positions_for_resolution((1920, 1080));
    let client = input::WindowRect { left: 100, top: 50, width: 1920, height: 1080 };
    let moved = layout_for((1920, 1080), Some(client), None);
    assert_eq!(moved.stash, (stock.stash.0 + 100.0, stock.stash.1 + 50.0));
    assert_eq!(moved.inv, (stock.inv.0 + 100.0, stock.inv.1 + 50.0));
    assert_eq!(moved.jump, stock.jump);

    let smaller = input::WindowRect { width: 1280, height: 720, ..client };
    assert_eq!(layout_for((1920, 1080), Some(smaller), None), stock, "a window of another size is ignored");
}

#[test]
fn a_calibration_saved_for_this_resolution_applies() {
    let calibration: input::CalibrationOverride = serde_json::from_value(serde_json::json!({
        "resolution": {"width": 3840, "height": 2160},
        "stashDelta": {"dx": 4.0, "dy": -2.0},
        "jump": 80.0
    }))
    .expect("calibration JSON");
    let layout = layout_for((3840, 2160), None, Some(&calibration));
    assert_eq!(layout.stash, (2760.0, 396.0));
    assert_eq!(layout.jump, 80.0);
    assert_eq!(layout_for((2560, 1440), None, Some(&calibration)), positions_for_resolution((2560, 1440)));
}
