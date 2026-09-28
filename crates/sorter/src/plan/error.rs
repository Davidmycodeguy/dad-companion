//! Errors the planner can raise. Port of `sort.py`'s `LayoutPlanError` and
//! `SortPlanningLimitExceeded`.

use thiserror::Error;

/// Raised when a deterministic plan cannot be created for the stash.
///
/// Python: `LayoutPlanError(Exception)`. There it carries a free-text message; here each cause
/// gets its own variant so callers can match on it instead of parsing strings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LayoutPlanError {
    /// The requested stash is the locked seasonal stash, or otherwise not a real stash tab
    /// (the bag or equipment slots). Nothing may plan into or out of it.
    #[error("stash {0} cannot be sorted")]
    StashNotSortable(u32),
    /// `state::grid_size` has no dimensions for this inventory id.
    #[error("stash {0} has no grid layout")]
    UnknownGrid(u32),
    /// Every candidate cell for the item was occupied or out of bounds.
    ///
    /// Python: `raise LayoutPlanError(f"Unable to place item '{itm}' within stash bounds")`.
    #[error("unable to place item {unique_id} within stash bounds")]
    NoRoomForItem { unique_id: u64 },
    /// The layout had a target for this item, but blocker resolution could not actually clear a
    /// path to it (every parking option — the bag, a future-safe stash cell, buffering something
    /// else — was exhausted). Python: `_move_item_to_target` returning `False`, logged and treated
    /// as an overall sort failure by `_execute_plan` rather than raised as an exception.
    #[error("could not clear a path to place item {unique_id}")]
    UnableToRelocate { unique_id: u64 },
}

/// Raised when simulated planning exceeds bounded resource limits.
///
/// Python: `SortPlanningLimitExceeded(Exception)`, constructed with a machine-readable `reason`
/// string and a user-facing `message`. The reason strings are kept identical (`code()`) since the
/// desktop app already matches on them (e.g. `"planning_move_budget_exceeded"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SortPlanningLimitExceeded {
    /// More moves were recorded than `PlanRequest::max_planned_moves` allows.
    #[error(
        "sort planning generated too many moves and was stopped before any items were moved"
    )]
    MoveBudgetExceeded,
    /// Planning ran longer than `PlanRequest::max_planning_duration` allows.
    #[error("sort planning took too long and was stopped before any items were moved")]
    Timeout,
}

impl SortPlanningLimitExceeded {
    /// The machine-readable reason string Python attaches to this exception.
    pub fn code(self) -> &'static str {
        match self {
            Self::MoveBudgetExceeded => "planning_move_budget_exceeded",
            Self::Timeout => "planning_timeout",
        }
    }
}

/// Everything that can go wrong building a [`super::SortPlan`]. Python does not have this
/// distinction — both exception types simply propagate to whatever called `StashSorter.sort()`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlanError {
    #[error(transparent)]
    Layout(#[from] LayoutPlanError),
    #[error(transparent)]
    LimitExceeded(#[from] SortPlanningLimitExceeded),
}
