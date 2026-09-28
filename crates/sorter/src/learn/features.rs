//! Bridges the adaptive model in [`super::model`] to this app's actual item and stash types
//! (`state::OwnedItem`, `game_data::Item`) by building the `ITEM_FEATURE_NAMES`-shaped feature
//! vector for one (item, candidate slot) pair.
//!
//! `sort_model.py` defines the feature *names* (`model::ITEM_FEATURE_NAMES`) but the code that
//! builds these vectors from live item/stash data lives outside the five Python modules this crate
//! ports (in `sort.py` / the layout planner, neither of which was part of this port's brief). This
//! module is therefore new code, not a line-for-line port: it fills in the same feature names with
//! definitions chosen to match what their names describe, so a model trained on vectors built here
//! predicts on the same footing it trains on.

use std::collections::HashMap;

use game_data::{Item, Rarity};
use state::{grid_size, slot_cell, OwnedItem};

use super::model::ITEM_FEATURE_NAMES;

/// Numeric rank for a rarity tier: higher is rarer. `Unknown` ranks *below* `Poor` (as "no
/// information available" rather than "rarer than Artifact") — treating an unrecognized rarity as
/// the rarest tier would bias the model toward over-protecting items it cannot even identify.
pub fn rarity_rank(rarity: Rarity) -> f64 {
    match rarity {
        Rarity::Unknown => -1.0,
        Rarity::Poor => 0.0,
        Rarity::Common => 1.0,
        Rarity::Uncommon => 2.0,
        Rarity::Rare => 3.0,
        Rarity::Epic => 4.0,
        Rarity::Legendary => 5.0,
        Rarity::Unique => 6.0,
        Rarity::Artifact => 7.0,
    }
}

/// One item already occupying the destination stash, used only to compute `blockers_at_target` and
/// `neighbor_rarity_match` — the two features that need to know about anything nearby.
pub struct PlacedItem {
    pub rarity: Rarity,
    pub top_left: (u32, u32),
    pub size: (u32, u32),
}

/// Everything needed to score one (item, candidate slot) placement: one call's worth of
/// `ITEM_FEATURE_NAMES`.
pub struct ItemFeatureInputs<'a> {
    pub catalog_item: &'a Item,
    pub owned: &'a OwnedItem,
    /// Stash the candidate slot is in (see `state::grid_size` for the ids this understands).
    pub inventory_id: u32,
    pub candidate_slot: (u32, u32),
    pub pack_mode: bool,
    pub stack_mode: bool,
    pub free_cells: u32,
    pub total_cells: u32,
    /// Other items already in the destination stash. Only entries near `candidate_slot` affect the
    /// result, so callers may pre-filter for speed; passing every item in the stash is also correct,
    /// just does more work than necessary.
    pub occupied: &'a [PlacedItem],
}

fn rectangles_overlap(a_pos: (u32, u32), a_size: (u32, u32), b_pos: (u32, u32), b_size: (u32, u32)) -> bool {
    let (ax0, ay0) = a_pos;
    let (ax1, ay1) = (ax0 + a_size.0, ay0 + a_size.1);
    let (bx0, by0) = b_pos;
    let (bx1, by1) = (bx0 + b_size.0, by0 + b_size.1);
    ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1
}

/// Whether `b` touches `a` — shares an edge or corner — without overlapping it. An item that
/// overlaps `a` is a blocker, not a neighbor, so it is explicitly excluded here even though it
/// would also satisfy the "grown rectangle" test below. The grown-rectangle test itself naturally
/// covers diagonal touches too, not just up/down/left/right.
fn rectangles_are_adjacent(a_pos: (u32, u32), a_size: (u32, u32), b_pos: (u32, u32), b_size: (u32, u32)) -> bool {
    if rectangles_overlap(a_pos, a_size, b_pos, b_size) {
        return false;
    }
    let grown_pos = (a_pos.0.saturating_sub(1), a_pos.1.saturating_sub(1));
    let grown_size = (a_size.0 + 2, a_size.1 + 2);
    rectangles_overlap(grown_pos, grown_size, b_pos, b_size)
}

/// Builds the `ITEM_FEATURE_NAMES`-shaped feature map for one (item, candidate slot) pair, for
/// [`super::SortAdaptiveModel::score_item_slot`] or for training-sample construction.
pub fn item_features(inputs: &ItemFeatureInputs) -> HashMap<String, f64> {
    let item = inputs.catalog_item;
    let width = f64::from(item.width);
    let height = f64::from(item.height);
    let candidate_size = (item.width.max(1), item.height.max(1));

    let (slot_x, slot_y) = inputs.candidate_slot;
    let (grid_w, grid_h) = grid_size(inputs.inventory_id).unwrap_or((1, 1));
    let slot_x_norm = if grid_w > 1 { f64::from(slot_x) / f64::from(grid_w - 1) } else { 0.0 };
    let slot_y_norm = if grid_h > 1 { f64::from(slot_y) / f64::from(grid_h - 1) } else { 0.0 };

    let free_ratio = if inputs.total_cells > 0 { f64::from(inputs.free_cells) / f64::from(inputs.total_cells) } else { 0.0 };

    // Only meaningful when the item is already sitting in this same stash; moving in from
    // elsewhere (the bag, another tab) has no "current" position to measure from.
    let distance_from_current = if inputs.owned.inventory_id == inputs.inventory_id {
        inputs.owned.slot_id.map_or(0.0, |slot_id| {
            let (cur_x, cur_y) = slot_cell(slot_id, grid_w);
            let dx = f64::from(cur_x) - f64::from(slot_x);
            let dy = f64::from(cur_y) - f64::from(slot_y);
            dx.hypot(dy)
        })
    } else {
        0.0
    };

    let blockers_at_target = inputs
        .occupied
        .iter()
        .filter(|placed| rectangles_overlap(inputs.candidate_slot, candidate_size, placed.top_left, placed.size))
        .count() as f64;

    let neighbors: Vec<&PlacedItem> = inputs
        .occupied
        .iter()
        .filter(|placed| rectangles_are_adjacent(inputs.candidate_slot, candidate_size, placed.top_left, placed.size))
        .collect();
    let neighbor_rarity_match = if neighbors.is_empty() {
        0.0
    } else {
        let matches = neighbors.iter().filter(|placed| placed.rarity == item.rarity).count();
        matches as f64 / neighbors.len() as f64
    };

    let mut features = HashMap::with_capacity(ITEM_FEATURE_NAMES.len());
    features.insert("width".to_string(), width);
    features.insert("height".to_string(), height);
    features.insert("area".to_string(), width * height);
    features.insert("max_side".to_string(), width.max(height));
    features.insert("rarity".to_string(), rarity_rank(item.rarity));
    features.insert("slot_x".to_string(), f64::from(slot_x));
    features.insert("slot_y".to_string(), f64::from(slot_y));
    features.insert("slot_x_norm".to_string(), slot_x_norm);
    features.insert("slot_y_norm".to_string(), slot_y_norm);
    features.insert("pack_mode".to_string(), f64::from(inputs.pack_mode));
    features.insert("stack_mode".to_string(), f64::from(inputs.stack_mode));
    features.insert("free_ratio".to_string(), free_ratio);
    features.insert("distance_from_current".to_string(), distance_from_current);
    features.insert("blockers_at_target".to_string(), blockers_at_target);
    features.insert("neighbor_rarity_match".to_string(), neighbor_rarity_match);
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_item(width: u32, height: u32, rarity: Rarity) -> Item {
        Item {
            id: "Potion_1001".to_string(),
            name: "Potion".to_string(),
            rarity,
            item_type: "Utility".to_string(),
            slot_type: String::new(),
            hand_type: String::new(),
            weapon_type: String::new(),
            armor_type: String::new(),
            utility_type: String::new(),
            tradable: true,
            max_stack: 1,
            width,
            height,
            vendor_price: 0,
            icon_path: None,
        }
    }

    fn sample_owned(inventory_id: u32, slot_id: Option<u32>) -> OwnedItem {
        OwnedItem {
            unique_id: 1,
            item_id: "Potion_1001".to_string(),
            count: 1,
            inventory_id,
            slot_id,
            base: Vec::new(),
            rolls: Vec::new(),
            loot_state: 0,
            tradable: true,
            contents: 0,
        }
    }

    #[test]
    fn rarity_rank_is_strictly_increasing_from_poor_to_artifact() {
        // Arrange
        let order = [
            Rarity::Poor,
            Rarity::Common,
            Rarity::Uncommon,
            Rarity::Rare,
            Rarity::Epic,
            Rarity::Legendary,
            Rarity::Unique,
            Rarity::Artifact,
        ];

        // Act / Assert
        for pair in order.windows(2) {
            assert!(rarity_rank(pair[0]) < rarity_rank(pair[1]));
        }
        assert!(rarity_rank(Rarity::Unknown) < rarity_rank(Rarity::Poor), "Unknown must rank below every known tier");
    }

    #[test]
    fn slot_normalization_does_not_divide_by_zero_for_a_gridless_inventory() {
        // Arrange: EQUIPMENT has no grid (`grid_size` returns `None`), so `item_features` falls
        // back to a 1x1 grid — exactly the case the `grid_w > 1` / `grid_h > 1` guards exist for.
        let item = sample_item(1, 1, Rarity::Common);
        let owned = sample_owned(state::stash::EQUIPMENT, None);
        let inputs = ItemFeatureInputs {
            catalog_item: &item,
            owned: &owned,
            inventory_id: state::stash::EQUIPMENT,
            candidate_slot: (0, 0),
            pack_mode: false,
            stack_mode: false,
            free_cells: 1,
            total_cells: 1,
            occupied: &[],
        };

        // Act
        let features = item_features(&inputs);

        // Assert
        assert_eq!(features["slot_x_norm"], 0.0);
        assert_eq!(features["slot_y_norm"], 0.0);
    }

    #[test]
    fn an_overlapping_item_counts_as_a_blocker_but_not_as_a_neighbor() {
        // Arrange: a 2x2 candidate at (0,0); one item overlapping it, one item merely touching it.
        let item = sample_item(2, 2, Rarity::Rare);
        let owned = sample_owned(state::stash::STORAGE, None);
        let overlapping = PlacedItem { rarity: Rarity::Common, top_left: (1, 1), size: (1, 1) };
        let touching = PlacedItem { rarity: Rarity::Rare, top_left: (2, 0), size: (1, 1) };
        let occupied = [overlapping, touching];
        let inputs = ItemFeatureInputs {
            catalog_item: &item,
            owned: &owned,
            inventory_id: state::stash::STORAGE,
            candidate_slot: (0, 0),
            pack_mode: false,
            stack_mode: false,
            free_cells: 100,
            total_cells: 240,
            occupied: &occupied,
        };

        // Act
        let features = item_features(&inputs);

        // Assert: one blocker (the overlapping item); the touching item is a neighbor whose rarity
        // matches the candidate's, so the match ratio is 1.0.
        assert_eq!(features["blockers_at_target"], 1.0);
        assert_eq!(features["neighbor_rarity_match"], 1.0);
    }

    #[test]
    fn distance_from_current_is_zero_when_the_item_has_no_current_slot_here() {
        // Arrange: item currently has no slot at all (e.g. fresh from the bag).
        let item = sample_item(1, 1, Rarity::Poor);
        let owned = sample_owned(state::stash::STORAGE, None);
        let inputs = ItemFeatureInputs {
            catalog_item: &item,
            owned: &owned,
            inventory_id: state::stash::STORAGE,
            candidate_slot: (5, 5),
            pack_mode: false,
            stack_mode: false,
            free_cells: 1,
            total_cells: 240,
            occupied: &[],
        };

        // Act
        let features = item_features(&inputs);

        // Assert
        assert_eq!(features["distance_from_current"], 0.0);
    }

    #[test]
    fn distance_from_current_measures_straight_line_cells_within_the_same_stash() {
        // Arrange: item currently at slot 0 (cell (0,0)) of STORAGE; candidate is cell (3,4).
        let item = sample_item(1, 1, Rarity::Poor);
        let owned = sample_owned(state::stash::STORAGE, Some(0));
        let inputs = ItemFeatureInputs {
            catalog_item: &item,
            owned: &owned,
            inventory_id: state::stash::STORAGE,
            candidate_slot: (3, 4),
            pack_mode: false,
            stack_mode: false,
            free_cells: 1,
            total_cells: 240,
            occupied: &[],
        };

        // Act
        let features = item_features(&inputs);

        // Assert: a 3-4-5 right triangle.
        assert_eq!(features["distance_from_current"], 5.0);
    }
}
