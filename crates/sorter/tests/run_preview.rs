//! The sorted-stash preview: every item at its planned cell, merged stacks added up, and the counts
//! the page shows next to the Sort button.

mod run_fakes;

use std::collections::HashMap;

use run_fakes::*;
use sorter::plan::{Location, World};
use sorter::run::build_preview;
use state::stash::{BAG, STORAGE};

#[test]
fn every_stash_item_is_shown_at_its_planned_cell_in_reading_order() {
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let world = World::build(&character, &catalog(), STORAGE).expect("storage loads");

    let preview = build_preview(&plan, &world);

    assert_eq!(preview.items.len(), 7);
    for item in &preview.items {
        assert_eq!(Location::new(STORAGE, item.cell), plan.positions[&item.unique_id]);
    }
    let order: Vec<(u32, u32)> = preview.items.iter().map(|i| (i.cell.y, i.cell.x)).collect();
    let mut reading = order.clone();
    reading.sort_unstable();
    assert_eq!(order, reading);
    assert_eq!(preview.items.iter().filter(|i| (i.width, i.height) == (2, 2)).count(), 2);
    assert_eq!((preview.drags, preview.incoming, preview.merges), (plan.moves.len(), 0, 0));
    assert!(preview.moving > 0 && preview.moving <= 7);
}

#[test]
fn merged_stacks_add_up() {
    // Three stacks of 2 (stack size 5): one merges into another, making 4 and 2.
    let character = character(vec![
        owned(1, "Potion_1", 2, STORAGE, 40),
        owned(2, "Potion_1", 2, STORAGE, 90),
        owned(3, "Potion_1", 2, STORAGE, 13),
        owned(4, "Ring_1", 1, STORAGE, 5),
    ]);
    let plan = plan_for(&character, true, Vec::new());
    let world = World::build(&character, &catalog(), STORAGE).expect("storage loads");

    let preview = build_preview(&plan, &world);

    let quantities: HashMap<u64, u32> = preview.items.iter().map(|i| (i.unique_id, i.quantity)).collect();
    assert_eq!(quantities, HashMap::from([(1, 4), (2, 2), (4, 1)]));
    assert_eq!(preview.merges, 1);
}

#[test]
fn items_from_the_bag_are_incoming_and_their_drags_counted() {
    let character = character(vec![
        owned(1, "Shield_1", 1, STORAGE, 50),
        owned(2, "Gem_1", 1, BAG, 3),
        owned(3, "Dagger_1", 1, BAG, 0),
        owned(4, "Ring_1", 1, BAG, 9),
    ]);
    let plan = plan_for(&character, false, vec![2, 3]);
    let world = World::build(&character, &catalog(), STORAGE).expect("storage loads");

    let preview = build_preview(&plan, &world);

    assert_eq!(preview.items.len(), 3, "the ring not brought over stays in the bag");
    assert_eq!(preview.incoming, 2);
    assert!(preview.bag_drags >= 2);
    assert!(preview.items.iter().filter(|i| i.from.inventory_id == BAG).all(|i| i.unique_id == 2 || i.unique_id == 3));
}
