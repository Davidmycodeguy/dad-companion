//! Running a [`crate::plan::SortPlan`] in the game: every planned move becomes a real drag in the
//! game's stash window, checked on screen before the next one starts. Port of the execution half of
//! DnDTools' `StashSorter` (`_replay_planned_moves` and helpers), `move_verifier.py`, and the OS
//! polling half of `sort_safety.py`.
//!
//! Start at [`RunSteps::new`] (the plan as a list of drags, validated against the stash it was
//! built from) and [`run_sort`] (the loop that performs them). Everything that decides something —
//! where a cell is on screen, whether a move happened, whether the stash on screen is the one the
//! plan expects, whether the player touched the mouse — is a pure function or a small state machine
//! here, so it is tested with a fake mouse and a fake screen (see `tests/run_*.rs`); only
//! [`Desktop`] and [`input::WindowsInput`] touch the real machine.
//!
//! # Safety rules this module enforces
//! - The locked Seasonal Shared Stash (`state::LOCKED_SEASONAL_STASH`) is refused outright: no step
//!   may start or end in it, and its tab is never clicked ([`tab_index`]).
//! - Before the first drag, the stash on screen must match the stash the plan was built from
//!   ([`check_layout`]). A wrong tab, stale data or a misplaced grid stops the run before anything
//!   is clicked in the grid.
//! - Every drag is verified with before/after captures at its source and destination
//!   ([`compare_probes`]); an unconfirmed move is re-checked, retried once, then the run stops
//!   rather than carrying on over a layout it no longer knows.
//! - The run stops at once when cancelled, when the game loses focus (after a short grace), or when
//!   the cursor is not where the sorter left it ([`Safety`]).
//!
//! # Differences from the Python
//! - `move_verifier.py` existed but was never wired into the sort; here every move is verified.
//! - Python's mouse-interference strikes were reset after every move, so they could never add up
//!   to a cancel. [`Safety`] instead checks that the cursor stays where the sorter put it whenever
//!   it should be still, which reacts within a few milliseconds of the player touching the mouse.
//! - Captures are taken with the cursor parked on an empty cell away from the move, so an item
//!   tooltip or hover highlight never lands in a probe (Python nudged the cursor by 15 px).

mod executor;
mod layout;
mod layout_check;
mod learning;
mod machine;
mod preview;
mod report;
mod safety;
mod screen_map;
mod speed;
mod steps;
mod tabs;
mod verify;

pub use executor::{run_sort, RunConfig, Timing};
pub use layout::layout_for;
pub use layout_check::{check_layout, CellPatch, LayoutCheck, LayoutVerdict};
pub use learning::{plan_metrics, session_features, LearningRecorder, MoveOutcomeSink};
pub use machine::{Desktop, Machine};
pub use preview::{build_preview, PreviewItem, SortPreview};
pub use report::{RunObserver, RunPhase, RunReport, StepReport, StopReason};
pub use safety::{spawn_watchdog, GuardedSink, InterferenceGuard, Safety, Watchdog};
pub use screen_map::{GridGeometry, ScreenMap};
pub use speed::AdaptiveSpeed;
pub use steps::{Board, RunStep, RunSteps, StepKind};
pub use tabs::tab_index;
pub use verify::{compare_probes, mean_abs_diff, MoveCheck, CHANGE_THRESHOLD};
