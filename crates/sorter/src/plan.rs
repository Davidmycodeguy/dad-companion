//! Sort planning: which item goes where, and the ordered moves that get it there. Port of
//! DnDTools' `sort.py` planning classes (moving the mouse to execute a plan lives elsewhere).
//!
//! Start at [`PlanRequest`] and [`build_sort_plan`]: given a `Character`'s current items and an
//! `ItemCatalog`, it builds the target layout and the ordered [`Move`]s that reach it. Everything
//! else here is that function's supporting cast — the grid simulation ([`World`]/[`Grid`]), the
//! comparator a player's sort order compiles to ([`compare_items`]), and the placement and
//! blocker-resolution algorithms it runs in sequence (`layout`, `ordering`, `stacking`, `resolve`).

mod buffer;
mod error;
mod geometry;
mod grid;
mod item;
mod layout;
mod ordering;
mod resolve;
mod safety;
mod search_sort;
mod sort_order;
mod sorter;
mod stacking;
mod state;
mod world;

pub use buffer::InventoryBufferTracker;
pub use error::{LayoutPlanError, PlanError, SortPlanningLimitExceeded};
pub use geometry::{intersects, Cell, Location};
pub use grid::{Grid, GridSnapshot};
pub use item::{rarity_rank, SortItem};
pub use layout::{build_layout, priority_order};
pub use ordering::optimize as optimize_order;
pub use resolve::{
    ensure_area_available, ensure_initial_workspace, move_item_to_target, DEFAULT_MAX_BUFFER_MOVES,
    DEFAULT_WORKSPACE_MIN_FREE_CELLS,
};
pub use safety::{CancelReason, SortSafetyMonitor};
pub use search_sort::{search_result_sort_key, SearchResult, SearchResultKey};
pub use sort_order::{
    build_comparator, compare_items, default_sort_order, normalize_sort_order, parse_sort_order,
    SortDirection, SortDirective, SortField, SORTABLE_FIELDS,
};
pub use sorter::{build_sort_plan, PlanRequest, SortPlan, DEFAULT_MAX_PLANNED_MOVES};
pub use stacking::{StackInstruction, StackingEngine};
pub use state::{LayoutPlan, Move, PlanEntry, PlanState};
pub use world::{TrackedItem, World};
