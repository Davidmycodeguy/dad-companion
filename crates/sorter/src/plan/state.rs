//! Shared plan data: the target layout, and lookups over it that `BlockerResolver` and
//! `WorkspaceManager`-equivalent logic use to find a safe place to park an item mid-sort. Port of
//! `sort.py`'s `PlanEntry`, `LayoutPlan` and `PlanState`.

use std::collections::HashMap;

use super::geometry::{Cell, Location};
use super::grid::Grid;
use super::world::World;

/// One item's place in the plan: its target cell, and bookkeeping `ExecutionOrderOptimizer` fills
/// in. Port of `PlanEntry` (there `item` is the `Item` object itself; here, its id — `World`
/// already owns the item data, so nothing else needs duplicating).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanEntry {
    pub unique_id: u64,
    pub target: Cell,
    pub needs_move: bool,
    pub blockers_count: u32,
    pub is_swap_candidate: bool,
}

impl PlanEntry {
    pub fn new(unique_id: u64, target: Cell) -> Self {
        Self { unique_id, target, needs_move: true, blockers_count: 0, is_swap_candidate: false }
    }
}

/// The final layout every active item should end up in, in the stash being sorted, plus an
/// execution order for getting them there. Port of `LayoutPlan` (its `learning` field, ML
/// bookkeeping for the sort-preference model, has no equivalent here; see the crate's `learn`
/// module for that instead).
#[derive(Debug, Clone, Default)]
pub struct LayoutPlan {
    pub positions: HashMap<u64, Cell>,
    pub order: Vec<PlanEntry>,
}

/// Lookups derived from a [`LayoutPlan`] that blocker-resolution consults while relocating items:
/// each item's planned target, whether a cell is already claimed by a not-yet-placed item, and how
/// far the plan execution has gotten. Port of `PlanState` (its `order` field is not duplicated onto
/// this type — nothing under `resolve` needs anything from a `PlanEntry` beyond what `positions`,
/// `rank_lookup` and `cell_plan_rank` already give it).
#[derive(Debug, Clone, Default)]
pub struct PlanState {
    /// Every active item's planned target cell in the stash.
    pub positions: HashMap<u64, Cell>,
    /// Every cell a not-yet-superseded plan entry's target footprint covers, mapped to that
    /// entry's index in `LayoutPlan::order`.
    pub cell_plan_rank: HashMap<Cell, usize>,
    /// Each item's index in `LayoutPlan::order`.
    pub rank_lookup: HashMap<u64, usize>,
    /// The index of the entry currently being placed.
    pub current_plan_index: usize,
}

impl PlanState {
    /// Builds the lookups from a finished, ordered plan. Port of `PlanState.build_from_layout`.
    pub fn build_from_layout(plan: &LayoutPlan, world: &World) -> Self {
        let mut state = PlanState { positions: plan.positions.clone(), ..PlanState::default() };
        for (index, entry) in plan.order.iter().enumerate() {
            state.rank_lookup.insert(entry.unique_id, index);
            if let Some(item) = world.item(entry.unique_id) {
                for cell in Grid::footprint(entry.target, item.width, item.height) {
                    state.cell_plan_rank.insert(cell, index);
                }
            }
        }
        state
    }
}

/// One step of the plan's replay: relocating an item, or merging it into another item's stack.
/// Python records these as `(start_stash, start_pos, end_stash, end_pos, ...)` tuples, captured by
/// a mouse-macro stand-in during a simulation phase. There is no separate simulation phase to
/// stand in for here — planning *is* the simulation — so a `Move` is simply appended whenever an
/// item is relocated or merged while building the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// `unique_id` moves from `from` to `to`.
    Relocate { unique_id: u64, from: Location, to: Location },
    /// `unique_id` merges into `target_id`'s stack (at `target_id`'s own location) and then
    /// disappears — `to` is `target_id`'s location, kept explicit so a replay doesn't need to look
    /// it up.
    StackInto { unique_id: u64, from: Location, target_id: u64, to: Location },
}
