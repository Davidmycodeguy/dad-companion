//! The live simulation a plan is built against: the stash being sorted, the bag used as scratch
//! space, and where every tracked item currently sits. Loosely mirrors `sort.py`'s `StashSorter`
//! holding `self.stash`/`self.inv` as two `Storage` objects that reach into each other; here it is
//! one owned struct, since Rust has no equivalent of Python's free-form back-references.

use std::collections::{HashMap, HashSet};

use game_data::ItemCatalog;
use state::stash::{EQUIPMENT, LAST_PURCHASED_STORAGE, SHARED_STASH_SEASONAL, STORAGE};
use state::{grid_size, is_off_limits, slot_cell, Character, OwnedItem};

use super::error::LayoutPlanError;
use super::geometry::{Cell, Location};
use super::grid::Grid;
use super::item::SortItem;

/// Whether `inventory_id` is a real stash tab the planner may sort: excludes the bag, equipment,
/// and (via `is_off_limits`) the locked seasonal stash. Not in Python, which never guards
/// `StashSorter`'s constructor against a nonsensical target — this closes that gap.
fn is_sortable_stash(inventory_id: u32) -> bool {
    !is_off_limits(inventory_id)
        && inventory_id != EQUIPMENT
        && ((STORAGE..=LAST_PURCHASED_STORAGE).contains(&inventory_id) || inventory_id == SHARED_STASH_SEASONAL)
}

/// One item the planner is tracking, and where it currently sits.
#[derive(Debug, Clone)]
pub struct TrackedItem {
    pub item: SortItem,
    pub location: Location,
}

/// The stash being sorted, the bag beside it, and every item on either grid. Python: the relevant
/// slice of `StashSorter.__init__` plus `Storage.load`.
pub struct World {
    pub stash_id: u32,
    pub stash: Grid,
    pub bag_id: u32,
    pub bag: Grid,
    items: HashMap<u64, TrackedItem>,
}

impl World {
    /// Loads `stash_id` and the bag from `character`'s current items. Skips any item whose
    /// recorded slot would place it (partly) off either grid, same as `Storage.load`'s bounds
    /// check; an item overlapping another in-bounds item is kept, also matching Python (which has
    /// no such check — the grid cell simply ends up holding whichever item was loaded last).
    pub fn build(character: &Character, catalog: &ItemCatalog, stash_id: u32) -> Result<Self, LayoutPlanError> {
        if !is_sortable_stash(stash_id) {
            return Err(LayoutPlanError::StashNotSortable(stash_id));
        }
        let bag_id = state::stash::BAG;
        let (stash_w, stash_h) = grid_size(stash_id).ok_or(LayoutPlanError::UnknownGrid(stash_id))?;
        let (bag_w, bag_h) = grid_size(bag_id).ok_or(LayoutPlanError::UnknownGrid(bag_id))?;

        let mut world = World {
            stash_id,
            stash: Grid::new(stash_w, stash_h),
            bag_id,
            bag: Grid::new(bag_w, bag_h),
            items: HashMap::new(),
        };
        world.load_owned(&character.stash(stash_id), stash_id, catalog);
        world.load_owned(&character.stash(bag_id), bag_id, catalog);
        Ok(world)
    }

    fn load_owned(&mut self, owned_items: &[OwnedItem], inventory_id: u32, catalog: &ItemCatalog) {
        for owned in owned_items {
            let item = SortItem::from_owned(owned, catalog);
            // `Character::stash` always assigns a slot, so this is only a defensive fallback.
            let Some(slot_id) = owned.slot_id else { continue };
            let width = self.grid(inventory_id).map(Grid::width).unwrap_or_default();
            let cell = Cell::from(slot_cell(slot_id, width));
            let Some(grid) = self.grid(inventory_id) else { continue };
            if !grid.in_bounds(cell, item.width, item.height) {
                continue;
            }
            self.grid_mut(inventory_id).expect("checked above").place(item.unique_id, cell, item.width, item.height);
            self.items.insert(item.unique_id, TrackedItem { item, location: Location::new(inventory_id, cell) });
        }
    }

    pub fn grid(&self, inventory_id: u32) -> Option<&Grid> {
        if inventory_id == self.stash_id {
            Some(&self.stash)
        } else if inventory_id == self.bag_id {
            Some(&self.bag)
        } else {
            None
        }
    }

    pub fn grid_mut(&mut self, inventory_id: u32) -> Option<&mut Grid> {
        if inventory_id == self.stash_id {
            Some(&mut self.stash)
        } else if inventory_id == self.bag_id {
            Some(&mut self.bag)
        } else {
            None
        }
    }

    pub fn item(&self, unique_id: u64) -> Option<&SortItem> {
        self.items.get(&unique_id).map(|t| &t.item)
    }

    pub fn location(&self, unique_id: u64) -> Option<Location> {
        self.items.get(&unique_id).map(|t| t.location)
    }

    pub fn items(&self) -> impl Iterator<Item = &SortItem> {
        self.items.values().map(|t| &t.item)
    }

    /// Updates a tracked item's quantity in place, e.g. after a stacking merge grows it. Does
    /// nothing if `unique_id` isn't tracked.
    pub fn set_quantity(&mut self, unique_id: u64, quantity: u32) {
        if let Some(tracked) = self.items.get_mut(&unique_id) {
            tracked.item.quantity = quantity;
        }
    }

    /// Ids of every tracked item currently on `inventory_id` (the stash or the bag).
    pub fn ids_on(&self, inventory_id: u32) -> HashSet<u64> {
        self.items.values().filter(|t| t.location.inventory_id == inventory_id).map(|t| t.item.unique_id).collect()
    }

    /// Removes an item from tracking entirely (and its grid), e.g. once it has been merged into
    /// another item's stack.
    pub fn remove(&mut self, unique_id: u64) {
        if let Some(tracked) = self.items.remove(&unique_id) {
            if let Some(grid) = self.grid_mut(tracked.location.inventory_id) {
                grid.remove(tracked.location.cell, tracked.item.width, tracked.item.height);
            }
        }
    }

    /// Moves a tracked item's footprint from its current location to `to`. Returns `false` (and
    /// changes nothing) if `unique_id` isn't tracked — every call site already knows it is; this
    /// is defensive rather than an invariant `panic!`, since a plan should never crash the app that
    /// asked for it.
    pub fn relocate(&mut self, unique_id: u64, to: Location) -> bool {
        let Some((width, height, from)) = self.items.get(&unique_id).map(|t| (t.item.width, t.item.height, t.location))
        else {
            return false;
        };
        if let Some(grid) = self.grid_mut(from.inventory_id) {
            grid.remove(from.cell, width, height);
        }
        if let Some(grid) = self.grid_mut(to.inventory_id) {
            grid.place(unique_id, to.cell, width, height);
        }
        if let Some(tracked) = self.items.get_mut(&unique_id) {
            tracked.location = to;
        }
        true
    }
}

#[cfg(test)]
impl World {
    /// A minimal world (one stash, one bag, no items) for unit tests that don't need a real
    /// `Character`/`ItemCatalog`.
    pub fn for_tests(stash: Grid, bag: Grid) -> Self {
        World { stash_id: STORAGE, stash, bag_id: state::stash::BAG, bag, items: HashMap::new() }
    }

    /// Places a synthetic 1-max-stack Common item directly on `inventory_id`'s grid, bypassing
    /// `Character`/`ItemCatalog`. Returns `unique_id` for convenience.
    pub fn place_for_tests(&mut self, unique_id: u64, inventory_id: u32, cell: Cell, width: u32, height: u32) -> u64 {
        self.place_item_for_tests(
            SortItem {
                unique_id,
                item_id: format!("test-{unique_id}"),
                name: format!("Test Item {unique_id}"),
                rarity: game_data::Rarity::Common,
                slot_type: String::new(),
                width,
                height,
                quantity: 1,
                max_stack: 1,
            },
            inventory_id,
            cell,
        )
    }

    /// Places a fully custom synthetic item (e.g. a specific `item_id`/`rarity`/`quantity` for
    /// exercising the stacking engine), bypassing `Character`/`ItemCatalog`.
    pub fn place_item_for_tests(&mut self, item: SortItem, inventory_id: u32, cell: Cell) -> u64 {
        let unique_id = item.unique_id;
        let (width, height) = (item.width, item.height);
        if let Some(grid) = self.grid_mut(inventory_id) {
            grid.place(unique_id, cell, width, height);
        }
        self.items.insert(unique_id, TrackedItem { item, location: Location::new(inventory_id, cell) });
        unique_id
    }
}
