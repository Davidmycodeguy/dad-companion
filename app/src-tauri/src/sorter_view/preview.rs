//! The page's side-by-side grids: the stash as it is, and as it will be once sorted.

use std::collections::HashSet;

use serde::Serialize;

use game_data::ItemCatalog;
use sorter::plan::{Cell, SortItem, World};
use sorter::run::SortPreview;

/// One item drawn in a grid.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridItemView {
    unique_id: String,
    item_id: String,
    name: String,
    rarity: &'static str,
    icon: Option<String>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    count: u32,
    /// In the sorted grid: ends somewhere other than where it is now. In the bag: comes over into
    /// the stash.
    moves: bool,
}

/// What the page shows before a sort.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SortPreviewView {
    pub stash_label: String,
    /// Grid sizes in cells.
    pub grid: (u32, u32),
    pub bag_grid: (u32, u32),
    pub current: Vec<GridItemView>,
    pub bag: Vec<GridItemView>,
    /// Empty when no plan could be made (see `blocked`).
    pub sorted: Vec<GridItemView>,
    /// Drags the sort makes, of which merges and trips into or out of the bag.
    pub moves: usize,
    pub merges: usize,
    pub bag_moves: usize,
    /// Items that end up elsewhere, and items coming over from the bag.
    pub moving: usize,
    pub incoming: usize,
    /// Why Sort can't start right now; None when it can.
    pub blocked: Option<String>,
    /// Seconds since the game last sent this character.
    pub data_age_s: Option<u64>,
}

fn view(item: &SortItem, cell: Cell, count: u32, catalog: &ItemCatalog, moves: bool) -> GridItemView {
    GridItemView {
        unique_id: item.unique_id.to_string(),
        item_id: item.item_id.clone(),
        name: item.name.clone(),
        rarity: item.rarity.name(),
        icon: catalog.get(&item.item_id).and_then(|c| c.icon_path.clone()),
        x: cell.x,
        y: cell.y,
        width: item.width,
        height: item.height,
        count,
        moves,
    }
}

/// Every item on `inventory_id` as loaded into `world`; `flagged` items are marked as moving.
pub fn grid_items(world: &World, inventory_id: u32, catalog: &ItemCatalog, flagged: &HashSet<u64>) -> Vec<GridItemView> {
    let mut items: Vec<GridItemView> = world
        .items()
        .filter_map(|item| {
            let at = world.location(item.unique_id).filter(|loc| loc.inventory_id == inventory_id)?;
            Some(view(item, at.cell, item.quantity, catalog, flagged.contains(&item.unique_id)))
        })
        .collect();
    items.sort_by_key(|item| (item.y, item.x));
    items
}

/// The sorted stash, in reading order.
pub fn sorted_items(world: &World, preview: &SortPreview, catalog: &ItemCatalog) -> Vec<GridItemView> {
    preview
        .items
        .iter()
        .filter_map(|planned| {
            let item = world.item(planned.unique_id)?;
            let moves = planned.from.inventory_id != world.stash_id || planned.from.cell != planned.cell;
            Some(view(item, planned.cell, planned.quantity, catalog, moves))
        })
        .collect()
}
