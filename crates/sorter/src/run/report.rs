//! What a run reports: why it stopped, what each move did, and the hooks the app listens on while
//! it goes. Python spread this across `_failure_reason` strings, overlay log lines and the
//! `sortCompleted`/`sortCancelled` window events.

use super::layout_check::LayoutCheck;
use super::steps::{RunStep, StepKind};

/// Why a run stopped before finishing every move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The player pressed Stop or the stop hotkey.
    Cancelled,
    /// The game was out of focus for longer than the grace period.
    FocusLost,
    /// The cursor was not where the sorter left it: the player moved the mouse.
    MouseMoved,
    /// Dark and Darker is not running.
    GameNotFound,
    /// The game window did not come to the front.
    GameNotFocused,
    /// The stash on screen does not match the stash the plan was built from (wrong tab, stale
    /// data, or the grid is not where the layout says).
    LayoutMismatch,
    /// Nothing on screen can confirm which stash is open: the stash and bag have too few empty
    /// cells to compare against.
    LayoutUnreadable,
    /// A move could not be confirmed on screen, even after a re-check and one retry.
    NotVerified { step: usize, item: String },
    /// A move showed up on screen, then its spots changed again before the next move: the game
    /// probably refused it and put the item back.
    MoveUndone { step: usize, item: String },
    /// Reading the screen failed.
    Capture(String),
    /// A mouse or keyboard call failed.
    Input(String),
    /// The plan asks for something the sorter never does (the locked stash, an unknown stash, a
    /// move that does not match the stash).
    Refused(String),
}

impl StopReason {
    /// A stable machine-readable code, for learning metadata and logs (Python kept the same kind
    /// of string in `_failure_reason`).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::FocusLost => "safety_focus_lost",
            Self::MouseMoved => "safety_mouse_interference",
            Self::GameNotFound => "game_not_found",
            Self::GameNotFocused => "game_not_focused",
            Self::LayoutMismatch => "layout_mismatch",
            Self::LayoutUnreadable => "layout_unreadable",
            Self::NotVerified { .. } => "move_not_verified",
            Self::MoveUndone { .. } => "move_undone",
            Self::Capture(_) => "capture_failed",
            Self::Input(_) => "input_failed",
            Self::Refused(_) => "refused",
        }
    }

    /// One or two sentences for the player: what happened and what to do next.
    pub fn message(&self) -> String {
        match self {
            Self::Cancelled => "Stopped. Items already moved stay where they are.".into(),
            Self::FocusLost => "Stopped because the game lost focus. Keep Dark and Darker in front while the sorter runs.".into(),
            Self::MouseMoved => "Stopped because the mouse moved. Keep your hands off the mouse while the sorter runs.".into(),
            Self::GameNotFound => "Dark and Darker isn't running.".into(),
            Self::GameNotFocused => "The game window didn't come to the front. Click into the game once, then sort again.".into(),
            Self::LayoutMismatch => "The stash on screen doesn't match the app's copy. Open this stash tab in the game with the stash window in view, or reopen your character to refresh it, then try again.".into(),
            Self::LayoutUnreadable => "The sorter can't confirm the stash on screen: it needs a few empty cells in the stash or bag to compare against.".into(),
            Self::NotVerified { step, item } => format!(
                "Stopped at move {}: {item} didn't move on screen, even after a retry. Check the stash in the game before sorting again.",
                step + 1
            ),
            Self::MoveUndone { step, item } => format!(
                "Stopped after move {}: {item} moved back after it was dropped, so the game may have refused the move. Check the stash in the game before sorting again.",
                step + 1
            ),
            Self::Capture(err) => format!("Reading the screen failed: {err}"),
            Self::Input(err) => format!("Moving the mouse failed: {err}"),
            Self::Refused(why) => why.clone(),
        }
    }

    /// Whether the player (or a safety check on their behalf) stopped the run, as opposed to the
    /// run failing. Learning records a cancelled session differently from a failed one.
    pub fn is_cancel(&self) -> bool {
        matches!(self, Self::Cancelled | Self::FocusLost | Self::MouseMoved)
    }
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// What the run is doing between moves, for the page's status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    FocusingGame,
    CheckingStash,
    OpeningTab,
    Moving,
}

impl RunPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::FocusingGame => "Bringing the game to the front",
            Self::CheckingStash => "Checking the stash on screen",
            Self::OpeningTab => "Opening the stash tab",
            Self::Moving => "Moving items",
        }
    }
}

/// What one move did.
#[derive(Debug, Clone, PartialEq)]
pub struct StepReport {
    /// Position of the move in the plan, from 0.
    pub index: usize,
    pub unique_id: u64,
    pub item_id: String,
    pub item_name: String,
    pub kind: StepKind,
    /// Drags performed: 1, or 2 after a retry.
    pub attempts: u32,
    pub verified: bool,
    /// Mean pixel change at the source and the destination between before and after.
    pub source_diff: f64,
    pub dest_diff: f64,
    /// The drag delay used for the last attempt, in seconds.
    pub delay_s: f64,
    /// Verified moves in a row before this one.
    pub streak_before: u32,
    /// Manhattan distance of the move in cells (across grids, as Python measured it).
    pub distance_cells: u32,
    pub item_area: u32,
    pub from_inventory: u32,
    pub to_inventory: u32,
}

/// The outcome of a whole run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunReport {
    /// Moves in the plan.
    pub total: usize,
    /// Moves whose first drag began. Anything above zero means the game's stash has changed and
    /// the app's copy is stale until the game sends it again.
    pub started: usize,
    /// Moves dragged and checked (verified or not).
    pub done: usize,
    pub verified: usize,
    /// Moves that needed a second drag.
    pub retried: usize,
    /// Moves that could not be confirmed (the run stops at the first).
    pub failed: usize,
    /// Why the run stopped early; `None` when every move was made and verified.
    pub stop: Option<StopReason>,
}

impl RunReport {
    pub fn is_complete(&self) -> bool {
        self.stop.is_none() && self.done == self.total
    }
}

/// Hooks the app uses to follow a run. Every method has a no-op default.
pub trait RunObserver {
    fn phase(&mut self, _phase: RunPhase) {}
    /// The stash on screen was compared with the plan's starting layout (worth logging: the counts
    /// explain a mismatch).
    fn layout_checked(&mut self, _check: &LayoutCheck) {}
    fn step_started(&mut self, _step: &RunStep, _index: usize, _total: usize) {}
    fn step_finished(&mut self, _report: &StepReport) {}
}

impl RunObserver for () {}
