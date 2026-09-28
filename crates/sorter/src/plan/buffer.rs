//! Tracks items temporarily parked in the bag while a plan is being built. Port of
//! `InventoryBufferTracker`.

use std::collections::HashMap;

use super::geometry::Location;
use super::world::World;

/// Which items are currently "buffered" (parked in the bag as a blocker, or marked for transfer
/// into the stash), and the bag cells held for them. Port of `InventoryBufferTracker`.
///
/// `buffered` is a `Vec`, not a `HashSet`: Python's `buffered_items` is a regular dict, which
/// iterates in the order items were marked, and `prepare_transfer_plan` relies on that order when
/// picking which buffered item tops up which stash stack first. A `HashSet` would make that
/// choice depend on hash iteration order instead — arbitrary, and not reproducible across runs.
#[derive(Debug, Clone, Default)]
pub struct InventoryBufferTracker {
    buffered: Vec<u64>,
    /// The location reserved for each buffered item, captured when it was marked. Kept separate
    /// from `World::location` (which tracks *current* position) because release must free the
    /// cells that were reserved even if the item has since moved on — Python: the `_reserved` dict,
    /// keyed by `id(item)`, captured at mark time rather than re-read at unmark time.
    reserved: HashMap<u64, Location>,
    /// How many times each item has already been shuffled aside while making room for a blocker.
    /// Blocker resolution uses this to avoid bouncing the same item back and forth forever.
    pub blocker_move_counts: HashMap<u64, u32>,
}

impl InventoryBufferTracker {
    pub fn is_buffered(&self, unique_id: u64) -> bool {
        self.buffered.contains(&unique_id)
    }

    /// Buffered items in the order they were marked.
    pub fn buffered_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.buffered.iter().copied()
    }

    pub fn buffered_len(&self) -> usize {
        self.buffered.len()
    }

    /// Marks `unique_id` as parked and, if it is currently in the bag, reserves its cells so
    /// nothing else claims them while it waits. Port of `InventoryBufferTracker.mark`.
    pub fn mark(&mut self, unique_id: u64, world: &mut World) {
        if !self.buffered.contains(&unique_id) {
            self.buffered.push(unique_id);
        }
        self.blocker_move_counts.remove(&unique_id);
        self.reserve(unique_id, world);
    }

    /// Unmarks `unique_id` and releases whatever `mark` reserved for it. Port of
    /// `InventoryBufferTracker.unmark`.
    pub fn unmark(&mut self, unique_id: u64, world: &mut World) {
        self.buffered.retain(|&id| id != unique_id);
        self.blocker_move_counts.remove(&unique_id);
        self.release(unique_id, world);
    }

    /// Port of `_reserve_slots`: only items presently in the bag get reserved (an item parked by
    /// leaving it in the stash isn't "buffered" in the sense that needs protecting).
    fn reserve(&mut self, unique_id: u64, world: &mut World) {
        let bag_id = world.bag_id;
        let Some(location) = world.location(unique_id).filter(|l| l.inventory_id == bag_id) else {
            return;
        };
        let Some((width, height)) = world.item(unique_id).map(|item| (item.width, item.height)) else {
            return;
        };
        if let Some(bag) = world.grid_mut(bag_id) {
            bag.reserve(location.cell, width, height);
        }
        self.reserved.insert(unique_id, location);
    }

    /// Port of `_release_slots`.
    fn release(&mut self, unique_id: u64, world: &mut World) {
        let Some(location) = self.reserved.remove(&unique_id) else { return };
        let Some((width, height)) = world.item(unique_id).map(|item| (item.width, item.height)) else {
            return;
        };
        if let Some(grid) = world.grid_mut(location.inventory_id) {
            grid.release(location.cell, width, height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::geometry::Cell;
    use super::super::grid::Grid;

    /// A minimal stand-in world (one stash, one bag) for exercising the tracker without needing a
    /// full `Character`/`ItemCatalog`.
    fn tiny_world() -> World {
        World::for_tests(Grid::new(4, 4), Grid::new(4, 4))
    }

    #[test]
    fn marking_a_bag_item_reserves_its_cells_and_unmarking_frees_them() {
        let mut world = tiny_world();
        let id = world.place_for_tests(1, world.bag_id, Cell::new(0, 0), 1, 1);
        let mut tracker = InventoryBufferTracker::default();

        tracker.mark(id, &mut world);
        assert!(!world.bag.is_free(Cell::new(0, 0))); // occupied AND reserved
        assert!(world.bag.fits(Cell::new(0, 0), 1, 1, Some(id))); // still fits for itself though

        tracker.unmark(id, &mut world);
        world.remove(id);
        assert!(world.bag.is_free(Cell::new(0, 0)));
    }

    #[test]
    fn release_frees_the_cell_reserved_at_mark_time_even_after_the_item_moves() {
        let mut world = tiny_world();
        let id = world.place_for_tests(1, world.bag_id, Cell::new(0, 0), 1, 1);
        let mut tracker = InventoryBufferTracker::default();

        tracker.mark(id, &mut world); // reserves bag (0,0)
        let stash_id = world.stash_id;
        world.relocate(id, Location::new(stash_id, Cell::new(2, 2))); // moves away without unmarking first
        tracker.unmark(id, &mut world);

        // The stale reservation at the old bag cell is gone, not left dangling.
        let bag = world.grid(world.bag_id).expect("bag exists");
        assert!(bag.is_free(Cell::new(0, 0)));
    }
}
