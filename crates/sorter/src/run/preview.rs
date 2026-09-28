//! The stash as it will look once sorted, for the page's side-by-side preview. DnDTools rendered
//! its preview to an image (`stash_preview.py`); here the page draws it, so this only works out
//! where everything ends up and how big each stack will be.

use std::collections::HashMap;

use crate::plan::{Cell, Location, Move, SortPlan, World};

/// One item in the sorted stash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewItem {
    pub unique_id: u64,
    /// Top-left cell in the sorted stash.
    pub cell: Cell,
    pub width: u32,
    pub height: u32,
    /// Stack size once merges are done.
    pub quantity: u32,
    /// Where the item is now (the stash, or the bag for an item brought over).
    pub from: Location,
}

/// What sorting will do, in numbers and as a layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SortPreview {
    /// Every item in the sorted stash, top to bottom, then left to right.
    pub items: Vec<PreviewItem>,
    /// Drags the run will make.
    pub drags: usize,
    /// Drags that merge a stack into another.
    pub merges: usize,
    /// Drags into or out of the bag: items parked while others move, and items brought over.
    pub bag_drags: usize,
    /// Items that end up somewhere other than where they are now.
    pub moving: usize,
    /// Items that come over from the bag.
    pub incoming: usize,
}

/// Builds the preview of `plan`, which was built from the stash and bag that `world` loads.
pub fn build_preview(plan: &SortPlan, world: &World) -> SortPreview {
    let mut quantity: HashMap<u64, u32> = world.items().map(|item| (item.unique_id, item.quantity)).collect();
    let mut merges = 0;
    let mut bag_drags = 0;
    for planned in &plan.moves {
        let (from, to) = match *planned {
            Move::Relocate { from, to, .. } => (from, to),
            Move::StackInto { unique_id, from, target_id, to } => {
                let added = quantity.remove(&unique_id).unwrap_or(0);
                let max_stack = world.item(target_id).map_or(u32::MAX, |item| item.max_stack.max(1));
                let stack = quantity.entry(target_id).or_insert(0);
                *stack = stack.saturating_add(added).min(max_stack);
                merges += 1;
                (from, to)
            }
        };
        if from.inventory_id == world.bag_id || to.inventory_id == world.bag_id {
            bag_drags += 1;
        }
    }
    let mut items: Vec<PreviewItem> = plan
        .positions
        .iter()
        .filter_map(|(&unique_id, &target)| {
            let item = world.item(unique_id)?;
            Some(PreviewItem {
                unique_id,
                cell: target.cell,
                width: item.width,
                height: item.height,
                quantity: quantity.get(&unique_id).copied().unwrap_or(item.quantity),
                from: world.location(unique_id)?,
            })
        })
        .collect();
    items.sort_by_key(|item| (item.cell.y, item.cell.x, item.unique_id));
    let moving = items.iter().filter(|item| item.from != Location::new(world.stash_id, item.cell)).count();
    let incoming = items.iter().filter(|item| item.from.inventory_id == world.bag_id).count();
    SortPreview { items, drags: plan.moves.len(), merges, bag_drags, moving, incoming }
}
