//! The top-level planning entry point: builds a [`World`] from a character's items, runs stacking,
//! workspace preparation, layout, and blocker resolution, and returns the finished plan. Port of
//! the planning parts of `StashSorter` (`_build_sort_plan`, `_execute_plan`, and the simulation half
//! of `sort`). Everything about mouse execution, ML-scored placement, telemetry callbacks and the
//! safety monitor's OS polling is out of scope — see the crate root doc comment.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use game_data::ItemCatalog;
use state::Character;

use super::buffer::InventoryBufferTracker;
use super::error::{LayoutPlanError, PlanError, SortPlanningLimitExceeded};
use super::geometry::Location;
use super::item::SortItem;
use super::layout::{build_layout, priority_order};
use super::ordering;
use super::resolve::{ensure_initial_workspace, move_item_to_target, DEFAULT_MAX_BUFFER_MOVES, DEFAULT_WORKSPACE_MIN_FREE_CELLS};
use super::sort_order::{compare_items, default_sort_order, SortDirective};
use super::stacking::StackingEngine;
use super::state::{Move, PlanEntry, PlanState};
use super::world::World;

/// How many moves a plan may record before planning gives up rather than risk runaway output.
/// Port of `StashSorter.DEFAULT_MAX_PLANNED_MOVES`.
pub const DEFAULT_MAX_PLANNED_MOVES: u32 = 5000;

/// What to plan: the knobs Python spreads across `StashSorter`'s constructor and
/// `mark_items_for_transfer`.
pub struct PlanRequest<'a> {
    pub character: &'a Character,
    pub catalog: &'a ItemCatalog,
    /// Which stash tab to sort (never the bag, equipment, or the locked seasonal stash).
    pub stash_id: u32,
    /// Accepted for parity with `StashSorter(pack_mode=...)`; does not currently affect placement
    /// order — see the doc comment on `layout::build_layout`'s internal `find_slot_for` for why.
    pub pack_mode: bool,
    /// Whether duplicate stacks already in the stash should be merged before laying out.
    pub stack_mode: bool,
    /// The order to sort by. Defaults to [`default_sort_order`].
    pub sort_order: Vec<SortDirective>,
    /// Bag items to fold into the sort instead of leaving them untouched (Python:
    /// `mark_items_for_transfer`). An id not currently in the bag is ignored.
    pub transfer_from_bag: Vec<u64>,
    pub max_planned_moves: u32,
    /// If set, planning gives up past this wall-clock budget (Python: always 45s via
    /// `DEFAULT_MAX_PLANNING_SECONDS`). Defaults to `None`: a native implementation is fast enough
    /// that porting Python's fixed 45s literally would almost never trigger, and an unconditional
    /// wall-clock check has no place in a deterministic library by default — set this explicitly if
    /// the embedding app wants the same safety net Python always had.
    pub max_planning_duration: Option<Duration>,
}

impl<'a> PlanRequest<'a> {
    pub fn new(character: &'a Character, catalog: &'a ItemCatalog, stash_id: u32) -> Self {
        Self {
            character,
            catalog,
            stash_id,
            pack_mode: false,
            stack_mode: false,
            sort_order: default_sort_order(),
            transfer_from_bag: Vec::new(),
            max_planned_moves: DEFAULT_MAX_PLANNED_MOVES,
            max_planning_duration: None,
        }
    }
}

/// The finished plan: where every active item should end up, and the ordered moves that get it
/// there. Python spreads this across `LayoutPlan` plus `StashSorter._planned_moves`.
#[derive(Debug, Clone, Default)]
pub struct SortPlan {
    pub positions: HashMap<u64, Location>,
    pub order: Vec<PlanEntry>,
    pub moves: Vec<Move>,
}

/// Where an item started, as a scan key: stash items first, top to bottom, left to right.
type StartKey = (bool, u32, u32);
/// The scan key of an item whose start isn't known: after everything else.
const UNKNOWN_START: StartKey = (true, u32::MAX, u32::MAX);

/// Every item's starting scan key. Identical items are placed in this order, so a sort never
/// shuffles items that are all alike (the world's own iteration order is arbitrary).
fn starting_order(world: &World) -> HashMap<u64, StartKey> {
    world
        .items()
        .map(|item| {
            let key = world
                .location(item.unique_id)
                .map_or(UNKNOWN_START, |l| (l.inventory_id != world.stash_id, l.cell.y, l.cell.x));
            (item.unique_id, key)
        })
        .collect()
}

/// The plan for a tab that is already sorted: every item where the layout would put it. None when
/// anything would move (the full planning then runs).
fn already_in_place(world: &World, request: &PlanRequest, original: &HashMap<u64, StartKey>) -> Option<SortPlan> {
    let mut items: Vec<SortItem> =
        world.items().filter(|item| world.location(item.unique_id).map(|l| l.inventory_id) == Some(world.stash_id)).cloned().collect();
    items.sort_by_key(|item| original.get(&item.unique_id).copied().unwrap_or(UNKNOWN_START));
    let (width, height) = (world.stash.width(), world.stash.height());
    let order = request.sort_order.clone();
    let layout = build_layout(width, height, &items, |a, b| compare_items(a, b, &order))
        .or_else(|_| build_layout(width, height, &items, priority_order))
        .ok()?;
    let positions: HashMap<u64, Location> =
        items.iter().map(|item| (item.unique_id, Location::new(world.stash_id, layout.positions[&item.unique_id]))).collect();
    let in_place = items.iter().all(|item| world.location(item.unique_id) == positions.get(&item.unique_id).copied());
    in_place.then(|| SortPlan { positions, order: Vec::new(), moves: Vec::new() })
}

/// Builds a full sort plan for `request`. Port of `StashSorter._build_sort_plan` plus the
/// simulation-phase body of `StashSorter.sort`/`_execute_plan` (stacking, workspace preparation,
/// layout, execution-order optimization, then relocating every item in that order).
pub fn build_sort_plan(request: &PlanRequest) -> Result<SortPlan, PlanError> {
    let started_at = Instant::now();
    let mut world = World::build(request.character, request.catalog, request.stash_id)?;
    let original = starting_order(&world);
    if !request.stack_mode && request.transfer_from_bag.is_empty() {
        if let Some(plan) = already_in_place(&world, request, &original) {
            return Ok(plan);
        }
    }
    let mut buffer = InventoryBufferTracker::default();
    let mut moves = Vec::new();

    for &id in &request.transfer_from_bag {
        if world.location(id).map(|l| l.inventory_id) == Some(world.bag_id) {
            buffer.mark(id, &mut world);
        }
    }

    // Stash-internal merges are decided first (matching `StashSorter.__init__` running them at
    // construction, before transfer mode can mark anything), then transfer merges are prepended —
    // `StackingEngine::prepare_transfer_plan` already does the prepending Python's comment
    // ("Prepend so transfer stacking runs before any stash-internal stacking") describes.
    let mut stacking = StackingEngine::default();
    if request.stack_mode {
        stacking.prepare_stash_plan(&mut world);
    }
    if !request.transfer_from_bag.is_empty() {
        stacking.prepare_transfer_plan(&mut world, &mut buffer);
    }
    stacking.apply(&mut world, &mut moves);

    ensure_initial_workspace(&mut world, &mut buffer, &mut moves, DEFAULT_WORKSPACE_MIN_FREE_CELLS, DEFAULT_MAX_BUFFER_MOVES);
    check_limits(&moves, request, started_at)?;

    // Active items: everything still on the stash (survived stacking), plus anything buffered —
    // buffering only ever happens to a stash item (workspace prep, blocker parking) or a bag item
    // explicitly marked for transfer, so an untouched bag item is correctly never "active". Port of
    // `unique_items` in `StashSorter.sort`.
    let mut active: Vec<SortItem> = world
        .items()
        .filter(|item| world.location(item.unique_id).map(|l| l.inventory_id) == Some(world.stash_id) || buffer.is_buffered(item.unique_id))
        .cloned()
        .collect();
    // In starting order, so the layout places items that compare equal the way they already were.
    active.sort_by_key(|item| original.get(&item.unique_id).copied().unwrap_or(UNKNOWN_START));

    let (width, height) = (world.stash.width(), world.stash.height());
    let order = request.sort_order.clone();
    let layout = build_layout(width, height, &active, |a, b| compare_items(a, b, &order))
        .or_else(|_| build_layout(width, height, &active, priority_order))?;

    let mut plan_state = PlanState::build_from_layout(&layout, &world);
    let optimized_order = ordering::optimize(&world, &layout);

    let mut lineage = HashSet::new();
    for (index, entry) in optimized_order.iter().enumerate() {
        plan_state.current_plan_index = index;
        if world.location(entry.unique_id) == Some(Location::new(world.stash_id, entry.target)) {
            continue;
        }
        lineage.clear();
        let placed = move_item_to_target(&mut world, &mut buffer, &mut moves, &plan_state, entry.unique_id, entry.target, &mut lineage);
        if !placed {
            return Err(LayoutPlanError::UnableToRelocate { unique_id: entry.unique_id }.into());
        }
        check_limits(&moves, request, started_at)?;
    }

    let positions =
        layout.positions.into_iter().map(|(id, cell)| (id, Location::new(world.stash_id, cell))).collect();
    Ok(SortPlan { positions, order: optimized_order, moves })
}

fn check_limits(moves: &[Move], request: &PlanRequest, started_at: Instant) -> Result<(), SortPlanningLimitExceeded> {
    if moves.len() as u32 > request.max_planned_moves {
        return Err(SortPlanningLimitExceeded::MoveBudgetExceeded);
    }
    if let Some(budget) = request.max_planning_duration {
        if started_at.elapsed() > budget {
            return Err(SortPlanningLimitExceeded::Timeout);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::geometry::Cell;
    use state::stash::STORAGE;
    use state::OwnedItem;

    fn owned(unique_id: u64, item_id: &str, count: u32, slot_id: u32, inventory_id: u32) -> OwnedItem {
        OwnedItem {
            unique_id,
            item_id: item_id.to_string(),
            count,
            contents: 0,
            inventory_id,
            slot_id: Some(slot_id),
            base: Vec::new(),
            rolls: Vec::new(),
            loot_state: 0,
            tradable: true,
        }
    }

    fn catalog() -> ItemCatalog {
        ItemCatalog::from_json(
            r#"{
                "Sword_1": {"name": "Sword", "rarity": "Common", "inventoryWidth": 1, "inventoryHeight": 2},
                "Shield_1": {"name": "Shield", "rarity": "Rare", "inventoryWidth": 2, "inventoryHeight": 2}
            }"#,
        )
        .expect("valid catalog json")
    }

    #[test]
    fn plans_and_moves_every_item_exactly_once() {
        let catalog = catalog();
        // Two items dumped in arbitrary slots; STORAGE is 12 wide, so slot 5 is (5,0) and slot 12
        // is (0,1) — neither at the top-left corner a fresh layout would choose.
        let character = Character {
            items: vec![owned(1, "Sword_1", 1, 5, STORAGE), owned(2, "Shield_1", 1, 12, STORAGE)],
            ..Character::default()
        };
        let request = PlanRequest::new(&character, &catalog, STORAGE);

        let plan = build_sort_plan(&request).expect("plan succeeds");

        assert_eq!(plan.positions.len(), 2);
        assert!(plan.positions.values().all(|loc| loc.inventory_id == STORAGE));
        assert!(!plan.moves.is_empty()); // neither item started at (0,0)/(0,2)
    }

    #[test]
    fn a_full_tab_of_identical_items_already_in_place_needs_no_moves() {
        let catalog = ItemCatalog::from_json(
            r#"{"GoldCoinPile_1": {"name": "Gold Coin Pile", "rarity": "Common", "inventoryWidth": 1, "inventoryHeight": 1, "maxCount": 25}}"#,
        )
        .expect("valid catalog json");
        // A 12x20 tab holding 240 identical piles, each already in its own cell.
        let items = (0..240).map(|slot| owned(1_000 + u64::from(slot), "GoldCoinPile_1", 25, slot, STORAGE)).collect();
        let character = Character { items, ..Character::default() };

        let plan = build_sort_plan(&PlanRequest::new(&character, &catalog, STORAGE)).expect("plan succeeds");

        assert!(plan.moves.is_empty(), "{} moves for a tab that is already sorted", plan.moves.len());
    }

    #[test]
    fn identical_items_keep_their_order_so_only_the_misplaced_one_moves() {
        let catalog = catalog();
        // Three swords in a row, then a gap, then a fourth sword: sorting only closes the gap.
        let character = Character {
            items: vec![
                owned(4, "Sword_1", 1, 0, STORAGE),
                owned(3, "Sword_1", 1, 1, STORAGE),
                owned(2, "Sword_1", 1, 2, STORAGE),
                owned(1, "Sword_1", 1, 6, STORAGE),
            ],
            ..Character::default()
        };

        let plan = build_sort_plan(&PlanRequest::new(&character, &catalog, STORAGE)).expect("plan succeeds");

        let moved: Vec<u64> = plan.order.iter().map(|entry| entry.unique_id).filter(|id| plan.positions[id] != Location::new(STORAGE, current_cell(&character, *id))).collect();
        assert_eq!(moved, [1], "only the sword after the gap should move");
    }

    fn current_cell(character: &Character, unique_id: u64) -> Cell {
        let slot = character.items.iter().find(|i| i.unique_id == unique_id).and_then(|i| i.slot_id).unwrap_or(0);
        Cell { x: slot % 12, y: slot / 12 }
    }

    #[test]
    fn refuses_to_plan_the_locked_seasonal_stash() {
        let catalog = catalog();
        let character = Character::default();
        let request = PlanRequest::new(&character, &catalog, state::LOCKED_SEASONAL_STASH);

        let err = build_sort_plan(&request).unwrap_err();
        assert_eq!(err, PlanError::Layout(LayoutPlanError::StashNotSortable(state::LOCKED_SEASONAL_STASH)));
    }
}
