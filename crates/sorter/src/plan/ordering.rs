//! Reorders a finished layout's entries to minimize blocker resolution and total moves. Port of
//! `ExecutionOrderOptimizer`.

use std::collections::{HashMap, HashSet};

use super::geometry::Location;
use super::grid::Grid;
use super::state::{LayoutPlan, PlanEntry};
use super::world::World;

/// Reorders `plan.order` against `world`'s *current* occupancy (unlike building the layout itself,
/// which plans against an empty scratch grid): items already sitting at their target come first
/// (nothing to do), then items whose target is currently empty (sorted top-to-bottom,
/// left-to-right), then items whose target is occupied (sorted by how many items are in the way,
/// fewest first). Items detected in a swap cycle (A wants B's cell, B wants C's, ..., back to A)
/// are flagged `is_swap_candidate` for the mover to potentially special-case, though nothing in
/// this port currently reads that flag back out — see [`detect_swap_chains`]. Port of
/// `ExecutionOrderOptimizer.optimize`.
pub fn optimize(world: &World, plan: &LayoutPlan) -> Vec<PlanEntry> {
    let swap_participants: HashSet<u64> = detect_swap_chains(world, plan).into_iter().flatten().collect();

    let mut already_placed = Vec::new();
    let mut free_target = Vec::new();
    let mut blocked_target = Vec::new();

    for &entry in &plan.order {
        let mut entry = entry;
        if is_at_target(world, &entry) {
            entry.needs_move = false;
            already_placed.push(entry);
            continue;
        }
        entry.needs_move = true;
        entry.blockers_count = count_blockers(world, &entry);
        entry.is_swap_candidate = swap_participants.contains(&entry.unique_id);
        if entry.blockers_count == 0 {
            free_target.push(entry);
        } else {
            blocked_target.push(entry);
        }
    }

    free_target.sort_by_key(|e| (e.target.y, e.target.x));
    blocked_target.sort_by_key(|e| (e.blockers_count, e.target.y, e.target.x));

    already_placed.into_iter().chain(free_target).chain(blocked_target).collect()
}

fn is_at_target(world: &World, entry: &PlanEntry) -> bool {
    world.location(entry.unique_id) == Some(Location::new(world.stash_id, entry.target))
}

fn count_blockers(world: &World, entry: &PlanEntry) -> u32 {
    let Some(item) = world.item(entry.unique_id) else { return 0 };
    Grid::footprint(entry.target, item.width, item.height)
        .filter(|&cell| world.stash.occupant(cell).is_some_and(|occupant| occupant != entry.unique_id))
        .count() as u32
}

/// Detects cycles in the "item wants the cell the next item occupies" graph — e.g. a two-item swap,
/// or a longer chain A→B→C→A — which can be resolved in `chain.len() + 1` moves via one temporary
/// slot instead of `2 * chain.len()`. Only each target's *origin* cell is checked against the live
/// grid (not its full footprint), matching Python's own simplification. Port of
/// `ExecutionOrderOptimizer._detect_swap_chains`.
fn detect_swap_chains(world: &World, plan: &LayoutPlan) -> Vec<Vec<u64>> {
    let entry_ids: HashSet<u64> = plan.order.iter().map(|e| e.unique_id).collect();
    let mut next_in_chain: HashMap<u64, u64> = HashMap::new();
    for entry in &plan.order {
        if is_at_target(world, entry) {
            continue;
        }
        if let Some(occupant) = world.stash.occupant(entry.target) {
            if occupant != entry.unique_id && entry_ids.contains(&occupant) {
                next_in_chain.insert(entry.unique_id, occupant);
            }
        }
    }
    // Python walks a regular dict in insertion order; mirrored here by walking `plan.order` again
    // rather than the `HashMap`'s own (unspecified) iteration order.
    let start_nodes: Vec<u64> = plan.order.iter().map(|e| e.unique_id).filter(|id| next_in_chain.contains_key(id)).collect();

    let mut visited: HashSet<u64> = HashSet::new();
    let mut chains = Vec::new();
    for start in start_nodes {
        if visited.contains(&start) {
            continue;
        }
        let mut path = Vec::new();
        let mut path_set = HashSet::new();
        let mut current = Some(start);
        while let Some(id) = current {
            if visited.contains(&id) {
                break;
            }
            if path_set.contains(&id) {
                let cycle_start = path.iter().position(|&p| p == id).expect("id is in path_set");
                let chain = path[cycle_start..].to_vec();
                if chain.len() >= 2 {
                    chains.push(chain);
                }
                break;
            }
            path.push(id);
            path_set.insert(id);
            current = next_in_chain.get(&id).copied();
        }
        visited.extend(path_set);
    }
    chains
}

#[cfg(test)]
mod tests {
    use super::super::geometry::Cell;
    use super::super::grid::Grid;
    use super::super::state::PlanEntry;
    use super::*;

    #[test]
    fn items_already_at_their_target_are_marked_done_and_come_first() {
        let mut world = World::for_tests(Grid::new(3, 1), Grid::new(2, 1));
        let id = world.place_for_tests(1, world.stash_id, Cell::new(0, 0), 1, 1);
        let plan = LayoutPlan { positions: HashMap::from([(id, Cell::new(0, 0))]), order: vec![PlanEntry::new(id, Cell::new(0, 0))] };

        let optimized = optimize(&world, &plan);
        assert_eq!(optimized.len(), 1);
        assert!(!optimized[0].needs_move);
    }

    #[test]
    fn a_free_target_is_preferred_over_a_blocked_one() {
        let mut world = World::for_tests(Grid::new(3, 1), Grid::new(2, 1));
        let blocked = world.place_for_tests(1, world.stash_id, Cell::new(1, 0), 1, 1);
        let free = world.place_for_tests(2, world.stash_id, Cell::new(2, 0), 1, 1);
        let plan = LayoutPlan {
            positions: HashMap::from([(blocked, Cell::new(0, 0)), (free, Cell::new(1, 0))]),
            order: vec![PlanEntry::new(blocked, Cell::new(0, 0)), PlanEntry::new(free, Cell::new(1, 0))],
        };

        let optimized = optimize(&world, &plan);
        // `free`'s target (1,0) is exactly where `blocked` currently sits, so from world's point of
        // view `free`'s target is occupied by `blocked` (not itself) — but `blocked`'s own target
        // (0,0) is empty, so `blocked` is the free-target entry and comes first.
        assert_eq!(optimized[0].unique_id, blocked);
        assert_eq!(optimized[0].blockers_count, 0);
        assert_eq!(optimized[1].unique_id, free);
        assert_eq!(optimized[1].blockers_count, 1);
    }

    #[test]
    fn a_two_item_swap_is_detected_as_a_chain() {
        let mut world = World::for_tests(Grid::new(2, 1), Grid::new(2, 1));
        let a = world.place_for_tests(1, world.stash_id, Cell::new(0, 0), 1, 1);
        let b = world.place_for_tests(2, world.stash_id, Cell::new(1, 0), 1, 1);
        // a wants b's cell, and b wants a's cell.
        let plan = LayoutPlan {
            positions: HashMap::from([(a, Cell::new(1, 0)), (b, Cell::new(0, 0))]),
            order: vec![PlanEntry::new(a, Cell::new(1, 0)), PlanEntry::new(b, Cell::new(0, 0))],
        };

        let optimized = optimize(&world, &plan);
        assert!(optimized.iter().all(|e| e.is_swap_candidate));
    }
}
