//! Computes the ideal target layout for a set of items, and the initial execution order for it.
//! Port of `LayoutPlanner`.
//!
//! Python's placement search also folds in an optional ML "which slot does the player prefer"
//! score (`learning_manager`). That model lives in this crate's `learn` module, not here — see the
//! crate root doc comment for why planning and learning are split — so this only ports the
//! deterministic path: the one Python itself takes whenever no learning manager is supplied, which
//! in the desktop app today is always.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::error::LayoutPlanError;
use super::geometry::Cell;
use super::grid::Grid;
use super::item::{rarity_rank, SortItem};
use super::state::{LayoutPlan, PlanEntry};

/// Computes where every item in `items` should end up in a `width`x`height` stash, in the order
/// given by `less_than` (typically [`super::compare_items`] against the player's sort order).
/// Items are placed one at a time, largest-priority first, each into the first free-and-in-bounds
/// cell scanning top-to-bottom then left-to-right — a scratch layout computed from empty, not
/// constrained by anything's current position (Python: `LayoutPlanner.occupancy` starts zeroed on
/// every `build()`, independent of the live stash grid). Port of `LayoutPlanner.build`.
///
/// Errors with [`LayoutPlanError::NoRoomForItem`] the first time some item has nowhere left to go
/// — same as Python raising `LayoutPlanError`.
pub fn build_layout(
    width: u32,
    height: u32,
    items: &[SortItem],
    less_than: impl Fn(&SortItem, &SortItem) -> std::cmp::Ordering,
) -> Result<LayoutPlan, LayoutPlanError> {
    let mut ordered: Vec<&SortItem> = items.iter().collect();
    ordered.sort_by(|a, b| less_than(a, b));

    let mut occupancy = Grid::new(width, height);
    let mut positions = HashMap::with_capacity(items.len());
    for item in &ordered {
        let slot = find_slot_for(&occupancy, item.width, item.height)
            .ok_or(LayoutPlanError::NoRoomForItem { unique_id: item.unique_id })?;
        occupancy.place(item.unique_id, slot, item.width, item.height);
        positions.insert(item.unique_id, slot);
    }

    // Initial execution order: top-to-bottom, left-to-right by target, largest items within a
    // start cell first. `ExecutionOrderOptimizer` (ordering::optimize) reorders this properly once
    // the live grid is known; this is just a sane starting point. Port of the `sorted(...)` call
    // at the end of `LayoutPlanner.build` (there, over `items` — the caller's original order —
    // rather than `ordered`, which only matters for the stable tiebreak on ties; matched here by
    // also building from `items`).
    let mut order: Vec<PlanEntry> = items
        .iter()
        .map(|item| PlanEntry::new(item.unique_id, positions[&item.unique_id]))
        .collect();
    let area_of: HashMap<u64, u32> = items.iter().map(|i| (i.unique_id, i.area())).collect();
    order.sort_by_key(|entry| (entry.target.y, entry.target.x, Reverse(area_of[&entry.unique_id])));

    Ok(LayoutPlan { positions, order })
}

/// The first cell (scanning top-to-bottom, then left-to-right) a `width`x`height` item fits in.
///
/// Port note: Python's equivalent, `LayoutPlanner._find_slot_for`, also scores candidates by
/// adjacency to already-placed items and by an optional ML slot score, preferring dense packing
/// when `prefer_dense` is set. But it scans and scores *every* candidate cell and keeps the one
/// with the lowest `(ml_score, y, x, adjacency)` tuple — and `y`/`x` are unique per candidate and
/// scanned in increasing order, so they alone always decide the minimum before adjacency is ever
/// consulted. With no ML score (this port's scope), `prefer_dense`/adjacency therefore can never
/// change the outcome: the winner is always simply the first fitting cell in scan order. This
/// reproduces that observable behavior directly instead of carrying the dead tiebreak along.
fn find_slot_for(occupancy: &Grid, width: u32, height: u32) -> Option<Cell> {
    if width > occupancy.width() || height > occupancy.height() {
        return None;
    }
    let max_x = occupancy.width() - width;
    let max_y = occupancy.height() - height;
    for y in 0..=max_y {
        for x in 0..=max_x {
            let origin = Cell::new(x, y);
            if occupancy.fits(origin, width, height, None) {
                return Some(origin);
            }
        }
    }
    None
}

/// The deterministic fallback ordering Python's `_learning_sort_key` reduces to whenever no
/// learning-model score is available (`(1, 0.0) + priority_key(item)`, the leading pair being
/// identical for every item once there is no score to break the tie with): largest area first,
/// then longest side, then rarity, then height, then name — all descending except name. Port of
/// `LayoutPlanner._priority_key`. Use this as `build_layout`'s `less_than` when the caller has no
/// explicit sort order to apply (Python: `_build_sort_plan`'s second, comparator-less attempt).
pub fn priority_order(a: &SortItem, b: &SortItem) -> std::cmp::Ordering {
    let key = |item: &SortItem| {
        (Reverse(item.area()), Reverse(item.longest_side()), Reverse(rarity_rank(item.rarity)), Reverse(item.height))
    };
    key(a).cmp(&key(b)).then_with(|| a.name.cmp(&b.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_data::Rarity;

    fn item(unique_id: u64, width: u32, height: u32) -> SortItem {
        SortItem {
            unique_id,
            item_id: format!("test-{unique_id}"),
            name: format!("Item {unique_id}"),
            rarity: Rarity::Common,
            slot_type: String::new(),
            width,
            height,
            quantity: 1,
            max_stack: 1,
        }
    }

    #[test]
    fn places_items_top_left_first_regardless_of_input_order() {
        let items = vec![item(1, 1, 1), item(2, 1, 1)];
        let plan = build_layout(2, 1, &items, |_, _| std::cmp::Ordering::Equal).unwrap();
        assert_eq!(plan.positions[&1], Cell::new(0, 0));
        assert_eq!(plan.positions[&2], Cell::new(1, 0));
    }

    #[test]
    fn a_comparator_decides_which_item_claims_the_first_slot() {
        // A 2x1 and a 1x1 item in a 1-row, 3-wide stash: whichever sorts first gets (0,0).
        let items = vec![item(1, 1, 1), item(2, 2, 1)];
        let plan = build_layout(3, 1, &items, |a, b| b.unique_id.cmp(&a.unique_id)).unwrap(); // id desc: 2 before 1
        assert_eq!(plan.positions[&2], Cell::new(0, 0));
        assert_eq!(plan.positions[&1], Cell::new(2, 0));
    }

    #[test]
    fn errors_when_an_item_has_nowhere_to_go() {
        let items = vec![item(1, 2, 1), item(2, 2, 1)];
        let err = build_layout(3, 1, &items, |_, _| std::cmp::Ordering::Equal).unwrap_err();
        assert_eq!(err, LayoutPlanError::NoRoomForItem { unique_id: 2 });
    }

    #[test]
    fn priority_order_places_the_largest_item_first() {
        let items = vec![item(1, 1, 1), item(2, 2, 2)];
        let plan = build_layout(3, 2, &items, priority_order).unwrap();
        assert_eq!(plan.positions[&2], Cell::new(0, 0)); // the 2x2 claims the corner first
    }
}
