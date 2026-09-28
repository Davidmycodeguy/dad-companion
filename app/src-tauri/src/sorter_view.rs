//! The Stash sorter page: its options, the sorted preview, and runs that sort a stash tab in the
//! game with the mouse. Planning, running and learning live in the `sorter` crate; this is the thin
//! layer between it, the app's state and the page.

mod commands;
mod learning;
mod options;
mod preview;
mod runner;
mod status;

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use sorter::plan::{build_sort_plan, LayoutPlanError, PlanError, PlanRequest, SortPlan};
use state::stash::BAG;
use state::{grid_size, is_off_limits, Character};

use crate::state::AppState;
use learning::Learning;
use status::SorterStatus;

// A glob, so the helper items `#[tauri::command]` generates beside each command come along.
pub use commands::*;

/// Where the learning model and its event store live, under the app's data folder.
const LEARNING_FOLDER: &str = "sort_learning";
/// Planning gives up past this (Python always stopped at 45 s).
const PLANNING_BUDGET: Duration = Duration::from_secs(10);
/// The game marks items it supplied for a run (the loadout) with this loot state; it won't let
/// them into the stash, so they are never brought over (DnDTools skipped them the same way).
const SUPPLIED_LOOT_STATE: i32 = 1;
const WAITING_FOR_REFRESH: &str =
    "Waiting for the game to send this stash again after the last sort. Reopen your character in the game to refresh it.";

/// The sorter's state for the app's lifetime: the current or last run, and the learning model.
pub struct Sorter {
    shared: Arc<Shared>,
    learning: Option<Arc<Learning>>,
}

struct Shared {
    status: Arc<Mutex<SorterStatus>>,
    /// Characters whose stash a run changed, with a fingerprint of the data that run started from
    /// and the stash it sorted: until the game sends different data, the app's copy is stale.
    awaiting_refresh: Mutex<HashMap<String, (u64, u32)>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Sorter {
    /// `data_dir` is the app's data folder; learning keeps its files in a folder inside it. Never
    /// fails: without its event store, sorting works and learning is off.
    pub fn new(data_dir: &Path) -> Self {
        let learning = match Learning::open(&data_dir.join(LEARNING_FOLDER)) {
            Ok(learning) => Some(Arc::new(learning)),
            Err(err) => {
                log::warn!("sort learning is off: {err}");
                None
            }
        };
        let shared = Shared { status: Arc::new(Mutex::new(SorterStatus::idle())), awaiting_refresh: Mutex::new(HashMap::new()) };
        Sorter { shared: Arc::new(shared), learning }
    }

    pub fn is_running(&self) -> bool {
        lock(&self.shared.status).is_running()
    }

    /// Why this character can't be sorted yet because the last sort changed its stash and the game
    /// hasn't sent it since. Fresh data clears the wait, and is checked for items the player moved
    /// by hand after the sort (learning).
    fn refresh_block(&self, character: &Character) -> Option<String> {
        let mut waiting = lock(&self.shared.awaiting_refresh);
        let &(seen, stash_id) = waiting.get(&character.id)?;
        if seen == fingerprint(character) {
            return Some(WAITING_FOR_REFRESH.into());
        }
        waiting.remove(&character.id);
        drop(waiting);
        if let Some(learning) = &self.learning {
            let corrections = learning.check_corrections(character, stash_id);
            if corrections > 0 {
                log::info!("{corrections} items were moved by hand after the last sort; learned from them");
            }
        }
        None
    }
}

/// A fingerprint of everything the character owns and where: changes whenever the game sends a
/// different stash.
fn fingerprint(character: &Character) -> u64 {
    let mut rows: Vec<(u64, u32, Option<u32>, u32)> =
        character.items.iter().map(|i| (i.unique_id, i.inventory_id, i.slot_id, i.count)).collect();
    rows.sort_unstable();
    let mut hasher = DefaultHasher::new();
    rows.hash(&mut hasher);
    hasher.finish()
}

fn find_character(state: &AppState, character_id: &str) -> Result<Character, String> {
    let characters = state.characters.lock().map_err(|_| "characters unavailable")?;
    characters.iter().find(|c| c.id == character_id).cloned().ok_or_else(|| "Pick a character first.".to_string())
}

/// Refuses anything but one of the character's stash tabs, and the locked seasonal stash always.
fn check_stash(character: &Character, stash_id: u32) -> Result<(), String> {
    if is_off_limits(stash_id) {
        return Err("The locked seasonal stash is never sorted.".into());
    }
    if stash_id == BAG || grid_size(stash_id).is_none() {
        return Err("Only stash tabs can be sorted.".into());
    }
    if !character.stash_ids().contains(&stash_id) {
        return Err("This character has no such stash tab.".into());
    }
    Ok(())
}

/// Bag items that can go into the stash: everything but the run supplies the game provided.
fn bag_items_to_bring(character: &Character) -> Vec<u64> {
    character.items.iter().filter(|i| i.inventory_id == BAG && i.loot_state != SUPPLIED_LOOT_STATE).map(|i| i.unique_id).collect()
}

/// Plans a sort of `stash_id` with `options`.
fn plan(state: &AppState, character: &Character, stash_id: u32, options: &options::SortOptions) -> Result<SortPlan, String> {
    let mut request = PlanRequest::new(character, &state.catalog, stash_id);
    request.pack_mode = options.pack;
    request.stack_mode = options.stack;
    request.sort_order = options.directives();
    request.transfer_from_bag = if options.from_bag { bag_items_to_bring(character) } else { Vec::new() };
    request.max_planning_duration = Some(PLANNING_BUDGET);
    build_sort_plan(&request).map_err(|err| plan_error(&err))
}

fn plan_error(err: &PlanError) -> String {
    match err {
        PlanError::Layout(LayoutPlanError::StashNotSortable(_) | LayoutPlanError::UnknownGrid(_)) => "This stash can't be sorted.".into(),
        PlanError::Layout(LayoutPlanError::NoRoomForItem { .. }) => "Everything doesn't fit in this tab. Bring fewer items over from the bag.".into(),
        PlanError::Layout(LayoutPlanError::UnableToRelocate { .. }) => {
            "The sorter can't find a way to move everything with the free space there is. Free a few cells in the stash or bag and try again.".into()
        }
        PlanError::LimitExceeded(limit) => format!("{}. Free some space in the bag and try again.", capitalize(&limit.to_string())),
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
