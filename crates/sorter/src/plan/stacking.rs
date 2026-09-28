//! Merges stackable items before the main layout pass. Port of `StackingEngine`.
//!
//! Python computes almost the same consolidation twice (once for stash-internal stacks, once for
//! buffered transfer items) with only the input list differing; this factors that shared algorithm
//! into [`consolidate`] instead of duplicating it.

use std::collections::HashMap;

use game_data::Rarity;

use super::buffer::InventoryBufferTracker;
use super::state::Move;
use super::world::World;

/// One planned merge: `source` disappears into `target`'s stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackInstruction {
    pub source: u64,
    pub target: u64,
}

/// Finds stackable merges and, once [`StackingEngine::apply`] runs, removes the merged-away items
/// from the world and records the moves. Port of `StackingEngine` (its constructor's `move_fn`
/// telemetry hook has no equivalent; `apply` always uses `World::remove` directly).
#[derive(Debug, Clone, Default)]
pub struct StackingEngine {
    instructions: Vec<StackInstruction>,
}

impl StackingEngine {
    pub fn instructions(&self) -> &[StackInstruction] {
        &self.instructions
    }

    pub fn has_instructions(&self) -> bool {
        !self.instructions.is_empty()
    }

    /// Merges duplicate stacks already sitting in the stash. Port of `prepare_stash_plan`.
    pub fn prepare_stash_plan(&mut self, world: &mut World) {
        let candidates: Vec<u64> = world.ids_on(world.stash_id).into_iter().collect();
        self.instructions = consolidate(&candidates, world);
    }

    /// Merges buffered (transfer) items among themselves, then tops up existing stash stacks with
    /// whatever is left over; unmarks and releases anything fully merged away. Port of
    /// `prepare_transfer_plan`. Run *before* `prepare_stash_plan`'s instructions (if any) apply, as
    /// Python does ("Prepend so transfer stacking runs before any stash-internal stacking").
    pub fn prepare_transfer_plan(&mut self, world: &mut World, buffer: &mut InventoryBufferTracker) {
        let buffered: Vec<u64> = buffer.buffered_ids().collect();
        if buffered.is_empty() {
            return;
        }

        let mut transfer_instructions = consolidate(&buffered, world);
        let merged_away: Vec<u64> = transfer_instructions.iter().map(|i| i.source).collect();

        let remaining: Vec<u64> = buffered.into_iter().filter(|id| !merged_away.contains(id)).collect();
        let stash_ids: Vec<u64> = {
            let mut ids: Vec<u64> = world.ids_on(world.stash_id).into_iter().collect();
            ids.sort_unstable();
            ids
        };
        transfer_instructions.extend(top_up_existing_stacks(&stash_ids, &remaining, world));

        // Every source that ended up merged away (phase 1 into each other, phase 2 into an
        // existing stash stack) stops being "buffered": it no longer needs a bag slot of its own.
        for instruction in &transfer_instructions {
            buffer.unmark(instruction.source, world);
        }

        self.instructions = transfer_instructions.into_iter().chain(self.instructions.drain(..)).collect();
    }

    /// Commits every planned merge: records a [`Move::StackInto`] for each and removes the merged
    /// source from the world. Port of `StackingEngine.execute`/`_stack_item` (there also called
    /// during the "simulation phase"; here planning *is* the simulation, so there is no separate
    /// execution phase to run this from later).
    pub fn apply(&self, world: &mut World, moves: &mut Vec<Move>) {
        for instruction in &self.instructions {
            let (Some(from), Some(to)) = (world.location(instruction.source), world.location(instruction.target))
            else {
                continue; // already gone (e.g. a duplicate instruction) — nothing to do
            };
            moves.push(Move::StackInto { unique_id: instruction.source, from, target_id: instruction.target, to });
            world.remove(instruction.source);
        }
    }
}

/// Groups `candidate_ids` (only those with `max_stack > 1`) by item id and rarity and, within each
/// group of more than one, greedily merges the smaller stacks into the largest ones: the biggest
/// stacks become "targets", and each smaller stack goes into the first target with enough spare
/// room, or becomes an additional target itself if none has room. Bumps each surviving target's
/// quantity in `world` immediately (matching Python doing so at "prepare" time, not when a merge
/// is later applied). Shared by `prepare_stash_plan` and phase 1 of `prepare_transfer_plan`.
fn consolidate(candidate_ids: &[u64], world: &mut World) -> Vec<StackInstruction> {
    let mut groups: HashMap<(String, Rarity), Vec<u64>> = HashMap::new();
    for &id in candidate_ids {
        if let Some(item) = world.item(id) {
            if item.max_stack > 1 {
                groups.entry((item.item_id.clone(), item.rarity)).or_default().push(id);
            }
        }
    }

    let mut instructions = Vec::new();
    for mut stackables in groups.into_values() {
        if stackables.len() <= 1 {
            continue;
        }
        // Largest stacks first; ties broken by id for determinism (Python's tiebreak instead
        // follows heap-internal iteration order, an implementation accident this does not chase).
        stackables.sort_by_key(|&id| (std::cmp::Reverse(quantity_of(world, id)), id));

        let max_stack = world.item(stackables[0]).map(|i| i.max_stack).unwrap_or(1).max(1);
        let total_qty: u32 = stackables.iter().map(|&id| quantity_of(world, id)).sum();
        let required_stacks = stackables.len().min(total_qty.div_ceil(max_stack) as usize);

        let mut targets: Vec<u64> = stackables[..required_stacks].to_vec();
        let mut capacity: HashMap<u64, u32> =
            targets.iter().map(|&id| (id, max_stack.saturating_sub(quantity_of(world, id).min(max_stack)))).collect();

        for &extra in &stackables[required_stacks..] {
            let extra_qty = quantity_of(world, extra);
            let chosen = targets.iter().copied().find(|t| capacity.get(t).copied().unwrap_or(0) >= extra_qty);
            match chosen {
                Some(target) => {
                    instructions.push(StackInstruction { source: extra, target });
                    *capacity.get_mut(&target).expect("looked up via targets") -= extra_qty;
                    let bumped = (quantity_of(world, target) + extra_qty).min(max_stack);
                    world.set_quantity(target, bumped);
                }
                None => {
                    capacity.insert(extra, max_stack.saturating_sub(extra_qty.min(max_stack)));
                    targets.push(extra);
                }
            }
        }
    }
    instructions
}

/// Tops up existing stash stacks using not-yet-merged buffered items of the same kind. Port of
/// `prepare_transfer_plan`'s phase 2 ("merge inventory items into existing STASH stacks").
fn top_up_existing_stacks(stash_ids: &[u64], remaining_buffered: &[u64], world: &mut World) -> Vec<StackInstruction> {
    let mut by_key: HashMap<(String, Rarity), Vec<u64>> = HashMap::new();
    for &id in remaining_buffered {
        if let Some(item) = world.item(id) {
            if item.max_stack > 1 {
                by_key.entry((item.item_id.clone(), item.rarity)).or_default().push(id);
            }
        }
    }

    let mut consumed = std::collections::HashSet::new();
    let mut instructions = Vec::new();
    for &stash_id in stash_ids {
        let Some((max_stack, key)) =
            world.item(stash_id).filter(|i| i.max_stack > 1).map(|i| (i.max_stack, (i.item_id.clone(), i.rarity)))
        else {
            continue;
        };
        let mut capacity = max_stack.saturating_sub(quantity_of(world, stash_id).min(max_stack));
        if capacity == 0 {
            continue;
        }
        let Some(candidates) = by_key.get(&key) else { continue };
        for &candidate in candidates {
            if capacity == 0 || consumed.contains(&candidate) {
                continue;
            }
            let qty = quantity_of(world, candidate);
            if qty <= capacity {
                instructions.push(StackInstruction { source: candidate, target: stash_id });
                capacity -= qty;
                let bumped = (quantity_of(world, stash_id) + qty).min(max_stack);
                world.set_quantity(stash_id, bumped);
                consumed.insert(candidate);
            }
        }
    }
    instructions
}

fn quantity_of(world: &World, unique_id: u64) -> u32 {
    world.item(unique_id).map(|item| item.quantity).unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::super::geometry::Cell;
    use super::super::grid::Grid;
    use super::super::item::SortItem;
    use super::*;

    fn potion(unique_id: u64, quantity: u32) -> SortItem {
        SortItem {
            unique_id,
            item_id: "Potion_1".to_string(),
            name: "Potion".to_string(),
            rarity: Rarity::Common,
            slot_type: String::new(),
            width: 1,
            height: 1,
            quantity,
            max_stack: 5,
        }
    }

    #[test]
    fn a_smaller_stack_merges_into_the_larger_one() {
        let mut world = World::for_tests(Grid::new(4, 4), Grid::new(4, 4));
        let big = world.place_item_for_tests(potion(1, 4), world.stash_id, Cell::new(0, 0));
        let small = world.place_item_for_tests(potion(2, 1), world.stash_id, Cell::new(1, 0));

        let mut engine = StackingEngine::default();
        engine.prepare_stash_plan(&mut world);

        assert_eq!(engine.instructions(), &[StackInstruction { source: small, target: big }]);
        assert_eq!(world.item(big).unwrap().quantity, 5); // 4 + 1, capped at max_stack

        let mut moves = Vec::new();
        engine.apply(&mut world, &mut moves);
        assert_eq!(moves.len(), 1);
        assert!(world.item(small).is_none()); // merged away entirely
        assert!(world.item(big).is_some());
    }

    #[test]
    fn a_stack_too_big_for_any_single_target_becomes_its_own_target() {
        // Two nearly-full stacks (4/5 each) plus one that cannot fit into either alone.
        let mut world = World::for_tests(Grid::new(4, 4), Grid::new(4, 4));
        let first = world.place_item_for_tests(potion(1, 4), world.stash_id, Cell::new(0, 0));
        let second = world.place_item_for_tests(potion(2, 4), world.stash_id, Cell::new(1, 0));
        let leftover = world.place_item_for_tests(potion(3, 2), world.stash_id, Cell::new(2, 0));

        let mut engine = StackingEngine::default();
        engine.prepare_stash_plan(&mut world);

        // Nothing has room for a quantity-2 extra (each target only has 1 spare), so it survives.
        assert!(engine.instructions().is_empty());
        for id in [first, second, leftover] {
            assert!(world.item(id).is_some());
        }
    }

    #[test]
    fn transfer_plan_tops_up_an_existing_stash_stack_and_unmarks_the_source() {
        let mut world = World::for_tests(Grid::new(4, 4), Grid::new(4, 4));
        let stash_item = world.place_item_for_tests(potion(1, 3), world.stash_id, Cell::new(0, 0));
        let bag_id = world.bag_id;
        let buffered = world.place_item_for_tests(potion(2, 2), bag_id, Cell::new(0, 0));

        let mut buffer = InventoryBufferTracker::default();
        buffer.mark(buffered, &mut world);

        let mut engine = StackingEngine::default();
        engine.prepare_transfer_plan(&mut world, &mut buffer);

        assert_eq!(engine.instructions(), &[StackInstruction { source: buffered, target: stash_item }]);
        assert_eq!(world.item(stash_item).unwrap().quantity, 5);
        assert!(!buffer.is_buffered(buffered)); // topped up, no longer needs its own bag slot
    }
}
