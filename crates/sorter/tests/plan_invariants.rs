//! Invariant tests over generated stashes. The Python original has almost no tests for this module
//! (see `plan_ported.rs` for what does exist); these instead check properties that must hold for
//! *any* valid input, generated deterministically (see `tests/common/mod.rs`) rather than by hand.
//!
//! Every scenario checks, via [`build_and_check`]:
//! 1. every originally-present stash item ends up with exactly one target position (unless a
//!    stacking merge legitimately absorbed it into another item's stack);
//! 2. the target layout itself never overlaps or leaves the grid;
//! 3. replaying the recorded moves in order, on a simulated grid starting from the real initial
//!    layout, never overlaps at any intermediate step and ends up exactly at the target layout;
//! 4. (checked separately, in `placement_follows_the_requested_sort_order_for_uniform_sized_items`,
//!    where uniform item sizes make "the order" unambiguous to state as a single assertion) that
//!    placement follows the requested sort order;
//! 5. nothing is ever placed on, or read from, the locked seasonal stash.

mod common;

use std::collections::{HashMap, HashSet};

use common::{character_with, fill_completely, owned, replay_moves, scatter_items, stackable_stash, test_catalog, Rng};
use game_data::{ItemCatalog, Rarity};
use sorter::plan::{build_sort_plan, Grid, GridSnapshot, Move, PlanError, PlanRequest, SortPlan, World};
use state::stash::{BAG, LOCKED_SEASONAL_STASH, STORAGE};
use state::Character;

/// Builds a plan for `character` and asserts invariants (1), (2), (3) and (5) (see module doc);
/// returns the plan so a scenario can make additional, situation-specific assertions on it without
/// planning twice.
///
/// Returns `Err` only for [`PlanError::Layout`]'s relocation-failure variants — a stash packed
/// tightly enough with big items can be a genuinely hard bin-packing-and-shuffling puzzle that a
/// greedy heuristic (this one, or Python's — see `resolve.rs`'s module doc) cannot always solve,
/// even though a valid target layout exists. That is an acceptable, gracefully-typed outcome; a
/// panic, an overlap, or a corrupted final layout is not, and this still asserts against every one
/// of those regardless of whether planning ultimately succeeds.
fn build_and_check(character: &Character, catalog: &ItemCatalog, stash_id: u32, stack_mode: bool) -> Result<SortPlan, PlanError> {
    let initial = World::build(character, catalog, stash_id).expect("stash_id is a valid, sortable stash");
    let dims: HashMap<u64, (u32, u32)> = initial.items().map(|i| (i.unique_id, (i.width, i.height))).collect();
    let started_on_stash: HashSet<u64> = initial
        .items()
        .map(|i| i.unique_id)
        .filter(|&id| initial.location(id).map(|l| l.inventory_id) == Some(stash_id))
        .collect();
    let initial_stash = initial.stash.clone();
    let initial_bag = initial.bag.clone();

    let mut request = PlanRequest::new(character, catalog, stash_id);
    request.stack_mode = stack_mode;
    let plan = build_sort_plan(&request)?;

    // (5) never touches the locked seasonal stash.
    assert!(plan.positions.values().all(|loc| loc.inventory_id != LOCKED_SEASONAL_STASH));
    assert!(plan.moves.iter().all(|m| !move_touches(m, LOCKED_SEASONAL_STASH)));

    // (1) every item placed exactly once, except a stacking merge's source, which has no target of
    // its own (it disappears into the one it merged into).
    let merged_sources: HashSet<u64> =
        plan.moves.iter().filter_map(|m| match m { Move::StackInto { unique_id, .. } => Some(*unique_id), _ => None }).collect();
    for &id in &started_on_stash {
        if merged_sources.contains(&id) {
            assert!(!plan.positions.contains_key(&id), "item {id} was merged away but still has a target");
        } else {
            assert!(plan.positions.contains_key(&id), "item {id} has no target in the plan");
        }
    }
    assert_eq!(plan.positions.len(), started_on_stash.len() - merged_sources.len());

    // (2) the target layout itself never overlaps or leaves the grid.
    let mut expected = Grid::new(initial_stash.width(), initial_stash.height());
    for (&id, &loc) in &plan.positions {
        assert_eq!(loc.inventory_id, stash_id, "item {id}'s target is not even in the stash being sorted");
        let (w, h) = dims[&id];
        assert!(expected.fits(loc.cell, w, h, None), "target layout overlaps at item {id}");
        expected.place(id, loc.cell, w, h);
    }

    // (3) replaying the moves reaches exactly the target layout (replay_moves itself asserts no
    // overlap and no `from`/actual-position mismatch at every step along the way).
    let replayed = replay_moves(&initial_stash, &initial_bag, stash_id, &dims, &plan.moves);
    let diff = GridSnapshot::capture(&expected).diff(&replayed);
    assert!(diff.is_empty(), "replaying the moves did not reach the target layout: {diff:?}");

    Ok(plan)
}

fn move_touches(m: &Move, inventory_id: u32) -> bool {
    match *m {
        Move::Relocate { from, to, .. } => from.inventory_id == inventory_id || to.inventory_id == inventory_id,
        Move::StackInto { from, to, .. } => from.inventory_id == inventory_id || to.inventory_id == inventory_id,
    }
}

/// At most this fraction of a "completely full, mixed sizes" batch may legitimately fail to plan
/// at all (see `build_and_check`'s doc comment) before it indicates an actual regression rather
/// than a handful of hard instances.
const MAX_ACCEPTABLE_UNSOLVABLE_FRACTION: f64 = 0.2;

#[test]
fn empty_stash_plans_to_an_empty_layout() {
    let catalog = test_catalog();
    let character = character_with(Vec::new(), Vec::new());
    let plan = build_and_check(&character, &catalog, STORAGE, false).expect("an empty stash always plans");
    assert!(plan.positions.is_empty());
    assert!(plan.moves.is_empty());
}

#[test]
fn completely_full_mixed_size_stash_across_seeds() {
    let catalog = test_catalog();
    let seeds: Vec<u64> = (0..30).collect();
    let unsolvable = seeds
        .iter()
        .filter(|&&seed| {
            let mut rng = Rng::new(seed);
            let mut next_id = 1;
            let items = fill_completely(&mut rng, 12, 20, &mut next_id);
            let character = character_with(items, Vec::new());
            build_and_check(&character, &catalog, STORAGE, false).is_err()
        })
        .count();
    assert!(
        (unsolvable as f64) <= (seeds.len() as f64) * MAX_ACCEPTABLE_UNSOLVABLE_FRACTION,
        "{unsolvable}/{} completely-full seeds could not be planned at all — that's more than expected \
         even for a hard bin-packing instance and likely indicates a regression",
        seeds.len()
    );
}

#[test]
fn partially_filled_mixed_size_stash_across_seeds() {
    let catalog = test_catalog();
    for seed in 100..130u64 {
        let mut rng = Rng::new(seed);
        let mut next_id = 1;
        let items = scatter_items(&mut rng, 12, 20, 0.6, &mut next_id);
        let character = character_with(items, Vec::new());
        build_and_check(&character, &catalog, STORAGE, false).expect("60%-full stash always plans");
    }
}

#[test]
fn stackable_items_consolidate_and_still_satisfy_every_invariant() {
    let catalog = test_catalog();
    for seed in 200..230u64 {
        let mut rng = Rng::new(seed);
        let mut next_id = 1;
        let items = stackable_stash(&mut rng, 12, 20, 10, &mut next_id);
        let character = character_with(items, Vec::new());
        let plan = build_and_check(&character, &catalog, STORAGE, true).expect("10 sparse 1x1 stacks always plan");
        // With 10 separate 1-4-quantity stacks of a max-5 stackable, stacking has real work to do:
        // confirm it actually ran, not just that the (weaker) general invariants happened to hold.
        assert!(plan.moves.iter().any(|m| matches!(m, Move::StackInto { .. })), "seed {seed}: expected at least one merge");
    }
}

#[test]
fn a_stash_with_no_free_cells_still_plans_by_using_the_bag_as_workspace() {
    let catalog = test_catalog();
    let seeds: Vec<u64> = (300..320).collect();
    let mut solved = 0;
    for &seed in &seeds {
        let mut rng = Rng::new(seed);
        let mut next_id = 1;
        let items = fill_completely(&mut rng, 12, 20, &mut next_id); // zero free cells to start
        let character = character_with(items, Vec::new());
        let Ok(plan) = build_and_check(&character, &catalog, STORAGE, false) else { continue };
        assert!(
            plan.moves.iter().any(|m| matches!(m, Move::Relocate { to, .. } if to.inventory_id == BAG)),
            "seed {seed}: expected at least one item buffered into the bag to make room"
        );
        solved += 1;
    }
    assert!(
        (solved as f64) >= (seeds.len() as f64) * (1.0 - MAX_ACCEPTABLE_UNSOLVABLE_FRACTION),
        "only {solved}/{} zero-free-cell seeds were plannable — the bag-as-workspace path may be broken",
        seeds.len()
    );
}

/// Invariant (4): placement follows the requested sort order. Stated with uniform-sized (1x1)
/// items so bin-packing can't muddy the picture: with nothing but 1x1s and room to spare, the
/// first-fit-top-left placement order *is* exactly the comparator order (see the doc comment on
/// `layout::build_layout`'s `find_slot_for`), so reading targets in scanline order must reproduce
/// the default sort order's rarity-descending tiebreak (every other default field — width, height,
/// slot, name — is tied across these five items).
#[test]
fn placement_follows_the_requested_sort_order_for_uniform_sized_items() {
    let catalog = test_catalog();
    let by_rarity = [("Relic_1", Rarity::Artifact), ("Gem_1", Rarity::Legendary), ("Ring_1", Rarity::Uncommon), ("Potion_1", Rarity::Common), ("Coin_1", Rarity::Poor)];
    // Scattered starting slots, deliberately not already in rarity order.
    let items: Vec<_> =
        by_rarity.iter().enumerate().map(|(i, &(item_id, _))| owned(i as u64 + 1, item_id, 1, (i as u32 + 1) * 7, STORAGE)).collect();
    let character = character_with(items, Vec::new());

    let plan = build_and_check(&character, &catalog, STORAGE, false).expect("five sparse 1x1 items always plan");

    let mut by_position: Vec<_> = plan.positions.iter().collect();
    by_position.sort_by_key(|(_, loc)| (loc.cell.y, loc.cell.x));
    let rarities: Vec<u8> = by_position
        .iter()
        .map(|(&id, _)| {
            let (_, rarity) = by_rarity[(id - 1) as usize];
            sorter::plan::rarity_rank(rarity)
        })
        .collect();
    let mut sorted_desc = rarities.clone();
    sorted_desc.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(rarities, sorted_desc, "scanline order should read highest rarity to lowest under the default sort order");
}

/// Invariant (5), more pointedly than the blanket check every scenario already runs: a decoy item
/// that only exists on the locked seasonal stash is never adopted into the plan for a *different*
/// stash, even though `Character::items` holds both.
#[test]
fn a_decoy_item_on_the_locked_seasonal_stash_is_never_adopted_into_another_stash_plan() {
    let catalog = test_catalog();
    let decoy_id = 999;
    let mut items = vec![owned(1, "Ring_1", 1, 0, STORAGE)];
    items.push(owned(decoy_id, "Greatsword_1", 1, 0, LOCKED_SEASONAL_STASH));
    let character = Character { items, ..Character::default() };

    let plan = build_and_check(&character, &catalog, STORAGE, false).expect("one real item always plans");

    assert!(!plan.positions.contains_key(&decoy_id));
    assert!(plan.moves.iter().all(|m| match *m {
        Move::Relocate { unique_id, .. } => unique_id != decoy_id,
        Move::StackInto { unique_id, target_id, .. } => unique_id != decoy_id && target_id != decoy_id,
    }));
}
