//! Resolving items that block a target cell during plan execution: parking a blocker aside, or
//! clearing space by shuffling things into the bag or elsewhere in the stash. Port of
//! `BlockerResolver` and `WorkspaceManager`.
//!
//! Python splits this into two classes that hold mutable references to each other
//! (`BlockerResolver.park` calls `WorkspaceManager.create_workspace_for`, which calls back into
//! `BlockerResolver.rebalance_inventory`). Rust has no equivalent of that back-reference dance
//! without runtime-checked interior mutability, so this ports both classes' methods as plain
//! functions that thread `World`/`InventoryBufferTracker`/the recorded moves through explicitly —
//! the same recursion, without the cycle.

use std::collections::HashSet;

use super::buffer::InventoryBufferTracker;
use super::geometry::{intersects, Cell, Location};
use super::grid::Grid;
use super::item::rarity_rank;
use super::state::{Move, PlanState};
use super::world::World;

/// How many buffered items `rebalance_inventory` will try moving back to the stash before giving
/// up. Port of `rebalance_inventory`'s `max_attempts` default.
const REBALANCE_MAX_ATTEMPTS: u32 = 5;
/// How many stash items `create_workspace_for` will try buffering before giving up. Port of
/// `create_workspace_for`'s `max_moves`.
const WORKSPACE_MAX_CANDIDATE_MOVES: u32 = 10;
/// The free-cell target `ensure_initial_workspace` aims for by default. Port of
/// `WorkspaceManager.workspace_min_free_cells`'s default.
pub const DEFAULT_WORKSPACE_MIN_FREE_CELLS: u32 = 6;
/// How many items `ensure_initial_workspace` will buffer to reach that target. Port of
/// `ensure_initial_workspace`'s `max_buffer_moves` default.
pub const DEFAULT_MAX_BUFFER_MOVES: u32 = 8;

/// Relocates a tracked item and records the move, marking it buffered (Python: pairing a `.move()`
/// into the bag with `buffer_tracker.mark`). Does nothing if `unique_id` isn't tracked.
fn move_and_mark(world: &mut World, buffer: &mut InventoryBufferTracker, moves: &mut Vec<Move>, unique_id: u64, to: Location) {
    if let Some(from) = world.location(unique_id) {
        world.relocate(unique_id, to);
        buffer.mark(unique_id, world);
        moves.push(Move::Relocate { unique_id, from, to });
    }
}

/// As [`move_and_mark`], but unmarks instead (Python: pairing a `.move()` back into the stash with
/// `buffer_tracker.unmark`).
fn move_and_unmark(world: &mut World, buffer: &mut InventoryBufferTracker, moves: &mut Vec<Move>, unique_id: u64, to: Location) {
    if let Some(from) = world.location(unique_id) {
        world.relocate(unique_id, to);
        buffer.unmark(unique_id, world);
        moves.push(Move::Relocate { unique_id, from, to });
    }
}

/// The first stash cell (scanning bottom-to-top, right-to-left) `item` could sit in without
/// overlapping anything except itself and, optionally, a forbidden rectangle — and without being
/// `item`'s own current cell (a "safe slot" search is always looking for somewhere *else*). Port of
/// `BlockerResolver.find_safe_slot`.
fn find_safe_slot(world: &World, item: u64, forbidden: Option<(Cell, u32, u32)>) -> Option<Cell> {
    let (width, height) = world.item(item).map(|i| (i.width, i.height))?;
    let stash = &world.stash;
    if width > stash.width() || height > stash.height() {
        return None;
    }
    let current = world.location(item);
    let max_x = stash.width() - width;
    let max_y = stash.height() - height;
    for y in (0..=max_y).rev() {
        for x in (0..=max_x).rev() {
            let candidate = Cell::new(x, y);
            if let Some((forbidden_origin, fw, fh)) = forbidden {
                if intersects(candidate, width, height, forbidden_origin, fw, fh) {
                    continue;
                }
            }
            if current == Some(Location::new(world.stash_id, candidate)) {
                continue;
            }
            if stash.fits(candidate, width, height, Some(item)) {
                return Some(candidate);
            }
        }
    }
    None
}

/// As [`find_safe_slot`], but a cell is only a candidate when nothing with a *lower* plan rank than
/// `min_rank` claims it — i.e. it won't collide with an item that still needs to reach its own
/// planned target. Port of `BlockerResolver.find_slot_with_min_rank`.
fn find_slot_with_min_rank(world: &World, plan_state: &PlanState, item: u64, min_rank: usize) -> Option<Cell> {
    let (width, height) = world.item(item).map(|i| (i.width, i.height))?;
    let stash = &world.stash;
    if width > stash.width() || height > stash.height() {
        return None;
    }
    let max_x = stash.width() - width;
    let max_y = stash.height() - height;
    for y in (0..=max_y).rev() {
        for x in (0..=max_x).rev() {
            let origin = Cell::new(x, y);
            let fits = Grid::footprint(origin, width, height).all(|cell| {
                let rank_allows = match plan_state.cell_plan_rank.get(&cell) {
                    Some(&rank) => rank >= min_rank,
                    None => true,
                };
                let occupant_allows = match stash.occupant(cell) {
                    None => true,
                    Some(occupant) => occupant == item,
                };
                rank_allows && occupant_allows
            });
            if fits {
                return Some(origin);
            }
        }
    }
    None
}

/// A safe slot for an item that itself hasn't been placed yet this pass: anywhere not claimed by
/// something ranked at or after it (its own target) or before it (already relied upon). Port of
/// `BlockerResolver.find_future_safe_slot`.
fn find_future_safe_slot(world: &World, plan_state: &PlanState, item: u64) -> Option<Cell> {
    let next_index = plan_state.current_plan_index + 1;
    let min_rank = next_index.max(plan_state.rank_lookup.get(&item).copied().unwrap_or(next_index));
    find_slot_with_min_rank(world, plan_state, item, min_rank)
}

/// Tries moving up to `max_attempts` other buffered items back to the stash, to free a bag slot
/// for `blocker`. Returns whether `blocker` has a free bag slot by the time it's done (whether or
/// not it actually needed to move anything). Port of `BlockerResolver.rebalance_inventory`.
fn rebalance_inventory(
    world: &mut World,
    buffer: &mut InventoryBufferTracker,
    moves: &mut Vec<Move>,
    blocker: u64,
    max_attempts: u32,
) -> bool {
    let bag_id = world.bag_id;
    let has_bag_slot = |world: &World| {
        world.item(blocker).is_some_and(|i| world.bag.find_empty_slot(i.width, i.height).is_some())
    };
    if buffer.buffered_len() == 0 {
        return false;
    }

    let mut attempts = 0;
    for candidate in buffer.buffered_ids().collect::<Vec<_>>() {
        if attempts >= max_attempts {
            break;
        }
        if candidate == blocker || world.location(candidate).map(|l| l.inventory_id) != Some(bag_id) {
            continue;
        }
        let Some(stash_slot) = find_safe_slot(world, candidate, None) else { continue };
        let stash_id = world.stash_id;
        move_and_unmark(world, buffer, moves, candidate, Location::new(stash_id, stash_slot));
        attempts += 1;
        if has_bag_slot(world) {
            return true;
        }
    }
    has_bag_slot(world)
}

/// Parks `blocker` somewhere out of the way: a free bag slot (rebalancing the bag first if
/// needed), else a stash cell nothing still-unplaced needs, else by creating workspace (buffering
/// some other stash item to make the bag or stash yield a slot). Port of `BlockerResolver.park`
/// (its telemetry/logging on failure has no equivalent).
fn park(world: &mut World, buffer: &mut InventoryBufferTracker, moves: &mut Vec<Move>, plan_state: &PlanState, blocker: u64) -> bool {
    let bag_id = world.bag_id;
    let Some((width, height)) = world.item(blocker).map(|i| (i.width, i.height)) else { return false };

    let mut bag_slot = world.bag.find_empty_slot(width, height);
    if bag_slot.is_none() && rebalance_inventory(world, buffer, moves, blocker, REBALANCE_MAX_ATTEMPTS) {
        bag_slot = world.bag.find_empty_slot(width, height);
    }
    if let Some(slot) = bag_slot {
        move_and_mark(world, buffer, moves, blocker, Location::new(bag_id, slot));
        return true;
    }

    if let Some(slot) = find_future_safe_slot(world, plan_state, blocker) {
        let stash_id = world.stash_id;
        move_and_unmark(world, buffer, moves, blocker, Location::new(stash_id, slot));
        return true;
    }

    create_workspace_for(world, buffer, moves, plan_state, None, blocker)
}

/// Whether `unique_id` is already sitting exactly where the plan wants it — i.e. already sorted, so
/// disturbing it would be pure regression rather than progress. Not a Python concept: Python's
/// `create_workspace_for` has no such check (see its doc comment for what that costs).
fn is_settled(world: &World, plan_state: &PlanState, unique_id: u64) -> bool {
    plan_state.positions.get(&unique_id).is_some_and(|&target| world.location(unique_id) == Some(Location::new(world.stash_id, target)))
}

/// Buffers the smallest, least-rare stash items (skipping `target_item`/`blocking_item`, and
/// anything already [`is_settled`]) one at a time until one leaves a stash cell free for
/// `blocking_item`, or `WORKSPACE_MAX_CANDIDATE_MOVES` attempts are used up. Port of
/// `WorkspaceManager.create_workspace_for`.
///
/// Port note: Python's candidate list has no `is_settled` exclusion, so it can pick an item that is
/// already correctly placed and move it away purely as workspace collateral. Nothing ever revisits
/// that entry afterward (`_execute_plan` walks the plan once, front to back), so the item is left
/// permanently wrong — a real, if rare, bug. There is no shortage of legitimate (not-yet-placed)
/// candidates to buffer instead, so excluding settled ones costs nothing.
fn create_workspace_for(
    world: &mut World,
    buffer: &mut InventoryBufferTracker,
    moves: &mut Vec<Move>,
    plan_state: &PlanState,
    target_item: Option<u64>,
    blocking_item: u64,
) -> bool {
    let bag_id = world.bag_id;
    let mut candidates: Vec<u64> = world
        .ids_on(world.stash_id)
        .into_iter()
        .filter(|&id| id != blocking_item && Some(id) != target_item && !is_settled(world, plan_state, id))
        .collect();
    if candidates.is_empty() {
        return false;
    }
    candidates.sort_by_key(|&id| workspace_priority(world, id));

    let mut attempted = 0;
    for candidate in candidates {
        if attempted >= WORKSPACE_MAX_CANDIDATE_MOVES {
            break;
        }
        let Some((w, h)) = world.item(candidate).map(|i| (i.width, i.height)) else { continue };
        let Some(bag_slot) = world.bag.find_empty_slot(w, h) else { continue };

        move_and_mark(world, buffer, moves, candidate, Location::new(bag_id, bag_slot));
        attempted += 1;

        let Some((bw, bh)) = world.item(blocking_item).map(|i| (i.width, i.height)) else { continue };
        if let Some(reassigned) = world.stash.find_empty_slot(bw, bh) {
            let stash_id = world.stash_id;
            move_and_unmark(world, buffer, moves, blocking_item, Location::new(stash_id, reassigned));
            return true;
        }
    }
    false
}

/// Smallest area first, then least rare: the order `create_workspace_for`/`ensure_initial_workspace`
/// prefer to buffer items in, so the items most likely to be worth keeping visible in the stash are
/// disturbed last. Ties break by id for determinism — Python's tiebreak instead follows
/// heap-internal iteration order, an implementation accident this does not chase (see the crate's
/// other `sort_by_key(..., id)` calls for the same reasoning).
fn workspace_priority(world: &World, unique_id: u64) -> (u32, u8, u64) {
    match world.item(unique_id) {
        Some(item) => (item.area(), rarity_rank(item.rarity), unique_id),
        None => (0, 0, unique_id),
    }
}

/// If the stash has fewer than `min_free_cells` empty cells, buffers stash items (smallest and
/// least rare first, see [`workspace_priority`]) into the bag until it does, or until
/// `max_buffer_moves` items have been buffered. Port of `WorkspaceManager.ensure_initial_workspace`
/// (its `min_free_cells: Option<u32>` defaulting to `workspace_min_free_cells` collapses here to
/// the caller always passing an explicit value, e.g. [`DEFAULT_WORKSPACE_MIN_FREE_CELLS`]).
pub fn ensure_initial_workspace(
    world: &mut World,
    buffer: &mut InventoryBufferTracker,
    moves: &mut Vec<Move>,
    min_free_cells: u32,
    max_buffer_moves: u32,
) {
    let mut free_cells = world.stash.count_free_cells();
    if free_cells >= min_free_cells {
        return;
    }
    let mut candidates: Vec<u64> = world.ids_on(world.stash_id).into_iter().collect();
    candidates.sort_by_key(|&id| workspace_priority(world, id));

    let bag_id = world.bag_id;
    let mut buffered = 0;
    for id in candidates {
        if free_cells >= min_free_cells || buffered >= max_buffer_moves {
            break;
        }
        let Some((w, h)) = world.item(id).map(|i| (i.width, i.height)) else { continue };
        let Some(slot) = world.bag.find_empty_slot(w, h) else { continue };
        move_and_mark(world, buffer, moves, id, Location::new(bag_id, slot));
        buffered += 1;
        free_cells += w * h;
    }
}

/// Clears every cell of `item`'s footprint at `target`, relocating whatever is currently there.
/// Each occupant tries, in order: a free bag slot; a free stash slot elsewhere (preferring the bag
/// instead if it's already been shuffled once before, to avoid endlessly bouncing it in place, or
/// after rebalancing the bag); and if the stash has no free slot at all, buffering some other item
/// to make room (retrying this whole pass once that succeeds). Port of
/// `WorkspaceManager.ensure_area_available` (its `cancel_event` cooperative-cancellation parameter
/// has no equivalent — planning has nothing to wait on).
///
/// While this runs, `target`'s own footprint is held reserved on the stash grid (released again
/// before returning). Python has no equivalent of this reservation, and as a result has a latent
/// bug this deliberately fixes: nothing stops one occupant's relocation — whether a plain
/// stash-internal reshuffle or a knock-on `rebalance_inventory` move — from landing back on a cell
/// of this very footprint that an *earlier* occupant in the same pass had only just vacated, which
/// silently corrupts the grid (two items now sharing a cell) rather than actually clearing the way
/// for `item`.
pub fn ensure_area_available(
    world: &mut World,
    buffer: &mut InventoryBufferTracker,
    moves: &mut Vec<Move>,
    plan_state: &PlanState,
    item: u64,
    target: Cell,
) -> bool {
    let Some((width, height)) = world.item(item).map(|i| (i.width, i.height)) else { return false };
    if !world.stash.in_bounds(target, width, height) {
        return false;
    }
    world.stash.reserve(target, width, height);
    let cleared = clear_footprint(world, buffer, moves, plan_state, item, target);
    world.stash.release(target, width, height);
    cleared
}

/// The recursive part of [`ensure_area_available`], split out only so that function can hold
/// `item`'s dimensions once and reserve/release around every retry of this. Re-reads them itself
/// (a cheap lookup) rather than taking them as two more parameters on top of an already-long list.
fn clear_footprint(world: &mut World, buffer: &mut InventoryBufferTracker, moves: &mut Vec<Move>, plan_state: &PlanState, item: u64, target: Cell) -> bool {
    let Some((width, height)) = world.item(item).map(|i| (i.width, i.height)) else { return false };
    let mut moved: HashSet<u64> = HashSet::new();
    for cell in Grid::footprint(target, width, height) {
        let Some(occupant) = world.stash.occupant(cell) else { continue };
        if occupant == item || moved.contains(&occupant) {
            continue;
        }
        if !relocate_blocking_occupant(world, buffer, moves, occupant) {
            return if create_workspace_for(world, buffer, moves, plan_state, Some(item), occupant) {
                clear_footprint(world, buffer, moves, plan_state, item, target) // reservation still held
            } else {
                false
            };
        }
        moved.insert(occupant);
    }
    true
}

/// The per-occupant relocation `ensure_area_available` tries before giving up and asking
/// `create_workspace_for` to make room instead.
///
/// Port note: Python finds its stash fallback cell (`new_pos`) once, up front, then reuses it after
/// possibly calling `rebalance_inventory` — which can itself move a *different* item into exactly
/// that cell first. This re-derives the cell fresh, right before actually placing anything in it,
/// instead of trusting a value that earlier calls in this same function may have invalidated.
fn relocate_blocking_occupant(world: &mut World, buffer: &mut InventoryBufferTracker, moves: &mut Vec<Move>, occupant: u64) -> bool {
    let bag_id = world.bag_id;
    let Some((w, h)) = world.item(occupant).map(|i| (i.width, i.height)) else { return false };

    if let Some(slot) = world.bag.find_empty_slot(w, h) {
        move_and_mark(world, buffer, moves, occupant, Location::new(bag_id, slot));
        return true;
    }
    // Nothing to reshuffle to at all if the stash has no free cell either — mirrors Python's
    // `if new_pos:` guard, just without holding onto *which* cell across the mutating calls below.
    if world.stash.find_empty_slot(w, h).is_none() {
        return false;
    }

    let moved_before = buffer.blocker_move_counts.get(&occupant).copied().unwrap_or(0) >= 1;
    if moved_before {
        if let Some(slot) = world.bag.find_empty_slot(w, h) {
            move_and_mark(world, buffer, moves, occupant, Location::new(bag_id, slot));
            return true;
        }
    }
    if world.bag.find_empty_slot(w, h).is_none() && rebalance_inventory(world, buffer, moves, occupant, REBALANCE_MAX_ATTEMPTS) {
        if let Some(slot) = world.bag.find_empty_slot(w, h) {
            move_and_mark(world, buffer, moves, occupant, Location::new(bag_id, slot));
            return true;
        }
    }

    let Some(new_pos) = world.stash.find_empty_slot(w, h) else { return false };
    let stash_id = world.stash_id;
    move_and_unmark(world, buffer, moves, occupant, Location::new(stash_id, new_pos));
    *buffer.blocker_move_counts.entry(occupant).or_insert(0) += 1;
    true
}

/// Everything currently occupying `item`'s footprint at `target`, other than `item` itself, in the
/// order their cells are scanned (top-to-bottom, left-to-right; each id listed once even if its
/// footprint covers several of the target's cells). Port of `StashSorter._collect_blocking_items`
/// (there a `set`, so unordered; ordered here for reproducibility — nothing depends on Python's
/// particular iteration order, which is itself just hash-bucket order over object ids).
fn collect_blocking_items(world: &World, item: u64, target: Cell) -> Vec<u64> {
    let Some((width, height)) = world.item(item).map(|i| (i.width, i.height)) else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut blockers = Vec::new();
    for cell in Grid::footprint(target, width, height) {
        if let Some(occupant) = world.stash.occupant(cell) {
            if occupant != item && seen.insert(occupant) {
                blockers.push(occupant);
            }
        }
    }
    blockers
}

/// Moves `item` to `target` in the stash, first relocating whatever is in the way: an item already
/// mid-relocation (`lineage`) is parked instead of recursed into again (breaking a cycle); one with
/// a planned target of its own is recursively moved there (or parked if that fails); one already
/// sitting *at* its planned target but still overlapping `item`'s destination is parked outright
/// (the two plans have a genuine conflict). Once nothing is in the way, clears any stragglers via
/// `ensure_area_available` and performs the move. Port of `StashSorter._move_item_to_target`.
pub fn move_item_to_target(
    world: &mut World,
    buffer: &mut InventoryBufferTracker,
    moves: &mut Vec<Move>,
    plan_state: &PlanState,
    item: u64,
    target: Cell,
    lineage: &mut HashSet<u64>,
) -> bool {
    if lineage.contains(&item) {
        return park(world, buffer, moves, plan_state, item);
    }
    let stash_id = world.stash_id;
    if world.location(item) == Some(Location::new(stash_id, target)) {
        return true;
    }

    lineage.insert(item);
    for blocker in collect_blocking_items(world, item, target) {
        if lineage.contains(&blocker) {
            if !park(world, buffer, moves, plan_state, blocker) {
                lineage.remove(&item);
                return false;
            }
            continue;
        }

        let Some(&desired) = plan_state.positions.get(&blocker) else {
            if !park(world, buffer, moves, plan_state, blocker) {
                lineage.remove(&item);
                return false;
            }
            continue;
        };

        let blocker_at_desired = world.location(blocker) == Some(Location::new(stash_id, desired));
        if !blocker_at_desired {
            let moved = move_item_to_target(world, buffer, moves, plan_state, blocker, desired, lineage);
            if !moved && !park(world, buffer, moves, plan_state, blocker) {
                lineage.remove(&item);
                return false;
            }
        } else {
            let (bw, bh) = world.item(blocker).map(|i| (i.width, i.height)).unwrap_or((1, 1));
            let (iw, ih) = world.item(item).map(|i| (i.width, i.height)).unwrap_or((1, 1));
            if intersects(desired, bw, bh, target, iw, ih) && !park(world, buffer, moves, plan_state, blocker) {
                lineage.remove(&item);
                return false;
            }
        }
    }

    if !ensure_area_available(world, buffer, moves, plan_state, item, target) {
        lineage.remove(&item);
        return false;
    }

    let Some(from) = world.location(item) else {
        lineage.remove(&item);
        return false;
    };
    world.relocate(item, Location::new(stash_id, target));
    buffer.unmark(item, world);
    moves.push(Move::Relocate { unique_id: item, from, to: Location::new(stash_id, target) });
    lineage.remove(&item);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::grid::Grid;

    fn plan_state(positions: &[(u64, Cell)]) -> PlanState {
        PlanState { positions: positions.iter().copied().collect(), ..PlanState::default() }
    }

    #[test]
    fn moving_to_an_empty_target_just_moves() {
        let mut world = World::for_tests(Grid::new(3, 1), Grid::new(2, 1));
        let item = world.place_for_tests(1, world.stash_id, Cell::new(0, 0), 1, 1);
        let mut buffer = InventoryBufferTracker::default();
        let mut moves = Vec::new();
        let state = plan_state(&[(item, Cell::new(2, 0))]);

        assert!(move_item_to_target(&mut world, &mut buffer, &mut moves, &state, item, Cell::new(2, 0), &mut HashSet::new()));
        assert_eq!(world.location(item), Some(Location::new(world.stash_id, Cell::new(2, 0))));
        assert_eq!(moves.len(), 1);
    }

    #[test]
    fn a_blocker_with_its_own_free_target_is_relocated_there_first() {
        let mut world = World::for_tests(Grid::new(3, 1), Grid::new(2, 1));
        let mover = world.place_for_tests(1, world.stash_id, Cell::new(0, 0), 1, 1);
        let blocker = world.place_for_tests(2, world.stash_id, Cell::new(1, 0), 1, 1);
        let mut buffer = InventoryBufferTracker::default();
        let mut moves = Vec::new();
        // `mover` wants the blocker's cell; the blocker's own plan sends it further along, to a
        // cell that is currently free.
        let state = plan_state(&[(mover, Cell::new(1, 0)), (blocker, Cell::new(2, 0))]);

        assert!(move_item_to_target(&mut world, &mut buffer, &mut moves, &state, mover, Cell::new(1, 0), &mut HashSet::new()));
        assert_eq!(world.location(mover), Some(Location::new(world.stash_id, Cell::new(1, 0))));
        assert_eq!(world.location(blocker), Some(Location::new(world.stash_id, Cell::new(2, 0))));
        assert_eq!(moves.len(), 2);
    }

    #[test]
    fn a_two_item_swap_is_resolved_by_parking_one_through_the_bag() {
        let mut world = World::for_tests(Grid::new(2, 1), Grid::new(1, 1));
        let a = world.place_for_tests(1, world.stash_id, Cell::new(0, 0), 1, 1);
        let b = world.place_for_tests(2, world.stash_id, Cell::new(1, 0), 1, 1);
        let mut buffer = InventoryBufferTracker::default();
        let mut moves = Vec::new();
        // a wants b's cell, b wants a's cell: a genuine swap, only solvable via a temporary slot.
        let state = plan_state(&[(a, Cell::new(1, 0)), (b, Cell::new(0, 0))]);

        assert!(move_item_to_target(&mut world, &mut buffer, &mut moves, &state, a, Cell::new(1, 0), &mut HashSet::new()));
        assert_eq!(world.location(a), Some(Location::new(world.stash_id, Cell::new(1, 0))));
        assert_eq!(world.location(b), Some(Location::new(world.stash_id, Cell::new(0, 0))));
        assert_eq!(moves.len(), 3); // park a, move b, move a back in — see the doc comment above
        assert!(!buffer.is_buffered(a)); // no longer parked once it reaches its real target
    }

    #[test]
    fn ensure_initial_workspace_buffers_the_smallest_items_first_until_the_target_is_met() {
        let mut world = World::for_tests(Grid::new(3, 1), Grid::new(3, 1));
        let big = world.place_for_tests(1, world.stash_id, Cell::new(0, 0), 2, 1);
        let small = world.place_for_tests(2, world.stash_id, Cell::new(2, 0), 1, 1);
        let mut buffer = InventoryBufferTracker::default();
        let mut moves = Vec::new();

        ensure_initial_workspace(&mut world, &mut buffer, &mut moves, 1, DEFAULT_MAX_BUFFER_MOVES);

        // Only the 1x1 needed to move to free one cell; the 2x1 stays put.
        assert!(buffer.is_buffered(small));
        assert!(!buffer.is_buffered(big));
        assert_eq!(moves.len(), 1);
    }
}
