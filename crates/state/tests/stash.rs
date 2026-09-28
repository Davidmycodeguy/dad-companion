use state::stash::{BAG, EQUIPMENT, STORAGE};
use state::{grid_size, is_off_limits, slot_cell, Character, OwnedItem, LOCKED_SEASONAL_STASH};

fn item(unique_id: u64, inventory_id: u32, slot_id: Option<u32>) -> OwnedItem {
    OwnedItem {
        unique_id,
        item_id: "HeaterShield_5001".into(),
        count: 1,
        contents: 0,
        inventory_id,
        slot_id,
        base: vec![],
        rolls: vec![],
        loot_state: 0,
        tradable: true,
    }
}

#[test]
fn stashes_have_their_grids() {
    assert_eq!(grid_size(BAG), Some((10, 5)));
    assert_eq!(grid_size(STORAGE), Some((12, 20)));
    assert_eq!(grid_size(EQUIPMENT), None);
}

#[test]
fn slots_run_row_by_row() {
    assert_eq!(slot_cell(0, 12), (0, 0));
    assert_eq!(slot_cell(13, 12), (1, 1));
    assert_eq!(slot_cell(49, 10), (9, 4));
}

#[test]
fn the_locked_seasonal_stash_is_off_limits() {
    assert!(is_off_limits(LOCKED_SEASONAL_STASH));
    assert!(!is_off_limits(STORAGE) && !is_off_limits(BAG));
}

#[test]
fn items_without_a_slot_get_the_lowest_free_one() {
    let character = Character {
        items: vec![item(1, STORAGE, Some(0)), item(2, STORAGE, None), item(3, STORAGE, Some(2)), item(4, BAG, None)],
        ..Character::default()
    };
    let slots: Vec<(u64, Option<u32>)> = character.stash(STORAGE).iter().map(|i| (i.unique_id, i.slot_id)).collect();
    assert_eq!(slots, [(1, Some(0)), (2, Some(1)), (3, Some(2))]);
    assert_eq!(character.stash_ids(), [BAG, STORAGE]);
}

#[test]
fn gold_is_coins_at_face_value_and_what_containers_hold() {
    let coins = OwnedItem { item_id: "GoldCoins".into(), count: 25, ..item(1, STORAGE, Some(0)) };
    let bag = OwnedItem { item_id: "GoldCoinBag".into(), contents: 2500, ..item(2, STORAGE, Some(1)) };
    assert_eq!((coins.gold(), bag.gold(), item(3, STORAGE, None).gold()), (Some(25), Some(2500), None));
}
