//! The runners that click through Dark and Darker's Marketplace and merchant screens.
//!
//! Port of DnDTools' `src/models/marketplace_runner.py` ([`marketplace`]), `merchant_runner.py`
//! ([`merchant`]), `marketplace_input.py` ([`live`]), the `SortSafetyMonitor` of `sort_safety.py`
//! ([`safety`]) and the runner / hover-test factories in `app.py` ([`live`], [`hover`]).
//!
//! # Fakes, never the real mouse, in tests
//!
//! Every game interaction goes through a trait: [`GameInput`] (clicks, moves, drags, typing,
//! Escape, the cursor position), [`MarketplaceGame`] / [`MerchantGame`] (the game's answers, read
//! from its packets), [`SafetyCheck`] (has the player taken over?) and [`GameWindow`] (bringing the
//! game forward). Only [`live`] touches the real machine.
//!
//! # For the app
//!
//! [`live_runner`], [`live_merchant_runner`] and [`live_hover_test`] build what
//! [`crate::job::ListerJob`]'s factories return; [`LiveConfig`] is what the app passes in. Each run
//! first brings the game window forward, then clicks under a [`SafetyMonitor`] that cancels the run
//! once the player moves the mouse or the game loses focus.
//!
//! The runners confirm every step from the game's answers, so the packet reader must feed:
//! `S2C_MARKETPLACE_MY_ITEM_LIST_RES`, `S2C_MARKETPLACE_ITEM_LIST_RES`,
//! `S2C_MARKETPLACE_ITEM_REGISTER_RES` and `S2C_MARKETPLACE_TRANSFER_ITEMS_RES` to the matching
//! `handle_*` of the [`MarketplaceState`](crate::marketplace_state::MarketplaceState) given to
//! [`live_runner`]; `S2C_MERCHANT_QUEST_LIST_INFO_RES` and `S2C_MERCHANT_STOCK_SELL_BACK_RES` to the
//! [`MerchantState`](crate::merchant_state::MerchantState) given to [`live_merchant_runner`].
//!
//! # Rules every runner keeps
//!
//! - Only the entries passed in are listed, never above their approved price (a re-check before
//!   listing may only lower it).
//! - Existing listings are never cancelled: Create Listing is only clicked on a free spot, and
//!   Transfer All Items only on sold or expired listings.
//! - The locked Seasonal Shared Stash (`state::is_off_limits`) is never opened: entries from it are
//!   refused before anything is clicked.

use input::marketplace::MarketplaceLayout;

use crate::job::{CancelToken, ItemResult, RunReport};
use crate::plan::PlanEntry;

pub mod game;
pub mod hover;
pub mod live;
pub mod marketplace;
pub mod merchant;
pub mod safety;

pub use game::{MarketplaceGame, MerchantGame};
pub use live::{
    game_resolution, hover_test_in, live_hover_test, live_merchant_runner, live_runner, merchant_runner_in, runner_in,
    Environment, GameWindow, LiveConfig,
};
pub use marketplace::MarketplaceRunner;
pub use merchant::MerchantSaleRunner;
pub use safety::{Desktop, NullSafety, SafetyCheck, SafetyMonitor};

/// A screen point in physical pixels, as [`MarketplaceLayout`] hands them out.
pub type ScreenPoint = (i32, i32);

/// Pixels (on either axis) the cursor may be from the spot a runner last used before the runner
/// decides the player took the mouse.
pub const CURSOR_DEVIATION_PX: u32 = 120;
pub const MOUSE_MOVED: &str = "Stopped for safety: the mouse was moved";
/// [`SafetyCheck::reason`] once the game lost focus.
pub const UNFOCUSED_REASON: &str = "game_window_unfocused";
/// [`SafetyCheck::reason`] once the player kept moving the mouse.
pub const MOUSE_REASON: &str = "mouse_interference";
/// A run cancelled (Ctrl+F12 in the app) without a safety reason.
pub const CANCELLED: &str = "Cancelled";
/// Why an item in the locked Seasonal Shared Stash is refused.
pub const OFF_LIMITS_REASON: &str = "in the locked Seasonal Shared Stash, which is never touched";
/// Said when a safety checkpoint fails without a reason (DnDTools' fallback text).
const NO_REASON_TEXT: &str = "the game lost focus";

/// The player-facing text for a [`SafetyCheck::reason`] (`friendly_reason` in DnDTools).
pub fn friendly_reason(reason: &str) -> &str {
    match reason {
        UNFOCUSED_REASON => "the game lost focus",
        MOUSE_REASON => "the mouse was moved",
        other => other,
    }
}

/// True for a stash id the lister must never open or touch (the locked Seasonal Shared Stash).
pub fn is_off_limits_stash(stash_id: &str) -> bool {
    stash_id.trim().parse::<u32>().is_ok_and(state::is_off_limits)
}

/// An input call that failed, e.g. the cursor position couldn't be read. It ends the run the way an
/// unexpected exception ended it in DnDTools.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InputError(pub String);

impl InputError {
    pub fn new(message: impl Into<String>) -> Self {
        InputError(message.into())
    }
}

/// The mouse and keyboard as the runners use them (DnDTools' `InputDriver` / `MerchantInput`
/// protocols). [`live::LiveInput`] sends real input; tests record what would have been sent.
pub trait GameInput: Send + Sync {
    /// Clicks `point`.
    fn click(&self, point: ScreenPoint) -> Result<(), InputError>;
    /// Moves the cursor to `point` without clicking.
    fn move_to(&self, point: ScreenPoint) -> Result<(), InputError>;
    /// Picks up what is at `from`, glides to `to` and drops it there.
    fn drag(&self, from: ScreenPoint, to: ScreenPoint) -> Result<(), InputError>;
    /// Clears the focused text box and types `text` (digits only).
    fn clear_and_type(&self, text: &str) -> Result<(), InputError>;
    /// Presses Escape the way the game reads it.
    fn press_escape(&self) -> Result<(), InputError>;
    /// Where the cursor is now.
    fn position(&self) -> Result<ScreenPoint, InputError>;
}

/// Why a step ended a run early: `Stop` is DnDTools' `_Stop` (a planned stop with a message for the
/// player, optionally carrying results to report), `Failed` any other exception (a failed input).
#[derive(Debug)]
enum Halt {
    Stop { message: String, results: Vec<ItemResult> },
    Failed(String),
}

impl Halt {
    fn stop(message: impl Into<String>) -> Self {
        Halt::Stop { message: message.into(), results: Vec::new() }
    }

    /// `_stop_with(result, message)`: a stop that also reports `result`.
    fn stop_with(result: ItemResult, message: impl Into<String>) -> Self {
        Halt::Stop { message: message.into(), results: vec![result] }
    }

    /// Appends `suffix` to a stop's message (dropping any results it carried, as re-raising a new
    /// `_Stop` did); a failed input passes through unchanged.
    fn suffixed(self, suffix: &str) -> Self {
        match self {
            Halt::Stop { message, .. } => Halt::stop(format!("{message}{suffix}")),
            failed => failed,
        }
    }
}

impl From<InputError> for Halt {
    fn from(error: InputError) -> Self {
        Halt::Failed(error.0)
    }
}

/// One runner step: `Err` ends the run.
type Step<T = ()> = Result<T, Halt>;

/// A run refused (or failed) before any item was processed.
fn refused(reason: impl Into<String>) -> RunReport {
    RunReport { results: Vec::new(), stopped_reason: Some(reason.into()) }
}

/// How DnDTools' job reported an exception a runner didn't catch itself.
fn unexpected(message: &str) -> String {
    format!("Unexpected error: {message}")
}

fn reason_of(safety: &dyn SafetyCheck) -> Option<String> {
    safety.reason().filter(|reason| !reason.is_empty())
}

/// `_check()`: stops a cancelled run, naming the safety reason when the monitor cancelled it.
fn check_cancel(cancel: &CancelToken, safety: &dyn SafetyCheck) -> Step {
    if !cancel.is_cancelled() {
        return Ok(());
    }
    Err(Halt::stop(match reason_of(safety) {
        Some(reason) => format!("Stopped for safety: {}", friendly_reason(&reason)),
        None => CANCELLED.to_string(),
    }))
}

/// `_safety_checkpoint()`: stops once the safety check says the player took over.
fn safety_checkpoint(safety: &dyn SafetyCheck) -> Step {
    if safety.checkpoint() {
        Ok(())
    } else {
        Err(Halt::stop(safety_stop_message(safety)))
    }
}

fn safety_stop_message(safety: &dyn SafetyCheck) -> String {
    let text = reason_of(safety).map(|reason| friendly_reason(&reason).to_string());
    format!("Stopped for safety: {}", text.filter(|t| !t.is_empty()).as_deref().unwrap_or(NO_REASON_TEXT))
}

/// True when the cursor is more than [`CURSOR_DEVIATION_PX`] from `point` on either axis.
fn cursor_moved_from(driver: &dyn GameInput, point: ScreenPoint) -> Step<bool> {
    let (x, y) = driver.position()?;
    Ok(x.abs_diff(point.0) > CURSOR_DEVIATION_PX || y.abs_diff(point.1) > CURSOR_DEVIATION_PX)
}

fn off_limits_message(name: &str) -> String {
    format!("{name} is {OFF_LIMITS_REASON}.")
}

fn unmapped_message(name: &str) -> String {
    format!("Stash tab for {name} is not mapped in DnDTools settings.")
}

/// The centre of `entry` in the open stash tab or inventory.
fn item_centre(layout: &MarketplaceLayout, entry: &PlanEntry) -> Step<ScreenPoint> {
    let (Ok(slot), Ok(width), Ok(height)) =
        (i32::try_from(entry.slot_id), i32::try_from(entry.width), i32::try_from(entry.height))
    else {
        return Err(Halt::stop(format!("{} has an impossible stash position, so it was not touched.", entry.name)));
    };
    Ok(layout.item_centre(&entry.stash_id, slot, width, height))
}
