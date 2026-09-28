//! The commands the Stash sorter page calls.

use std::collections::HashSet;

use serde::Serialize;
use tauri::State;

use sorter::plan::World;
use sorter::run::{build_preview, RunSteps};
use state::stash::BAG;

use super::options::{self, SortOptions};
use super::preview::{grid_items, sorted_items, SortPreviewView};
use super::runner::{self, RunJob};
use super::status::{lock as lock_status, SorterStatus};
use super::{bag_items_to_bring, check_stash, find_character, fingerprint, plan, Sorter};
use crate::state::AppState;

/// Who the input lock names while the sorter holds it.
const SORTER: &str = "The stash sorter";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterOptionsView {
    options: SortOptions,
    /// The player confirmed the automated-input notice once already.
    risk_accepted: bool,
}

#[tauri::command]
pub fn sorter_options(state: State<'_, AppState>) -> Result<SorterOptionsView, String> {
    let settings = state.settings.lock().map_err(|_| "settings unavailable")?;
    Ok(SorterOptionsView { options: SortOptions::load(&settings), risk_accepted: settings.get(options::RISK_KEY).unwrap_or(false) })
}

/// Saves the options; the cleaned-up copy comes back.
#[tauri::command]
pub fn sorter_save_options(state: State<'_, AppState>, options: SortOptions) -> Result<SortOptions, String> {
    let clean = options.cleaned();
    let mut settings = state.settings.lock().map_err(|_| "settings unavailable")?;
    clean.save(&mut settings)?;
    Ok(clean)
}

/// The stash now and once sorted with `options`, with the move counts and anything that stops a
/// sort from starting.
#[tauri::command]
pub async fn sorter_preview(state: State<'_, AppState>, character_id: String, stash_id: u32, options: SortOptions) -> Result<SortPreviewView, String> {
    let character = find_character(&state, &character_id)?;
    check_stash(&character, stash_id)?;
    let world = World::build(&character, &state.catalog, stash_id).map_err(|err| format!("This stash can't be sorted: {err}."))?;
    let incoming: HashSet<u64> = if options.from_bag { bag_items_to_bring(&character).into_iter().collect() } else { HashSet::new() };
    let mut view = SortPreviewView {
        stash_label: crate::stash_view::stash_label(stash_id),
        grid: (world.stash.width(), world.stash.height()),
        bag_grid: (world.bag.width(), world.bag.height()),
        current: grid_items(&world, stash_id, &state.catalog, &HashSet::new()),
        bag: grid_items(&world, BAG, &state.catalog, &incoming),
        sorted: Vec::new(),
        moves: 0,
        merges: 0,
        bag_moves: 0,
        moving: 0,
        incoming: 0,
        blocked: state.sorter.refresh_block(&character),
        data_age_s: data_age_s(&state, &character_id),
    };
    match plan(&state, &character, stash_id, &options.cleaned()) {
        Ok(planned) => {
            let preview = build_preview(&planned, &world);
            view.sorted = sorted_items(&world, &preview, &state.catalog);
            (view.moves, view.merges, view.bag_moves) = (preview.drags, preview.merges, preview.bag_drags);
            (view.moving, view.incoming) = (preview.moving, preview.incoming);
        }
        Err(why) => view.blocked = view.blocked.or(Some(why)),
    }
    Ok(view)
}

/// Plans the sort again from the latest data and starts it in the game. Only from the page's Sort
/// button; `accept_risk` records the player's confirmation of the notice on their first sort.
#[tauri::command]
pub async fn sorter_start(state: State<'_, AppState>, character_id: String, stash_id: u32, options: SortOptions, accept_risk: bool) -> Result<(), String> {
    let character = find_character(&state, &character_id)?;
    check_stash(&character, stash_id)?;
    let options = options.cleaned();
    let (risk_accepted, learning_enabled) = {
        let mut settings = state.settings.lock().map_err(|_| "settings unavailable")?;
        if accept_risk {
            settings.set(options::RISK_KEY, true).map_err(|err| format!("could not save settings: {err}"))?;
        }
        options.save(&mut settings)?;
        (settings.get(options::RISK_KEY).unwrap_or(false), settings.get(options::LEARNING_KEY).unwrap_or(true))
    };
    if !risk_accepted {
        return Err("Confirm the notice about automated input first.".into());
    }
    if let Some(why) = state.sorter.refresh_block(&character) {
        return Err(why);
    }
    if state.sorter.is_running() {
        return Err("A sort is already running.".into());
    }
    let planned = plan(&state, &character, stash_id, &options)?;
    let steps = RunSteps::new(&character, &state.catalog, stash_id, &planned).map_err(|why| why.message())?;
    if steps.is_empty() {
        return Err("This stash is already sorted.".into());
    }
    if input::find_game_window().is_none() {
        return Err("Dark and Darker isn't running.".into());
    }
    let config = {
        let settings = state.settings.lock().map_err(|_| "settings unavailable")?;
        runner::screen_config(&settings, &character, stash_id)
    };
    let guard = state.input_lock.try_acquire(SORTER)?;
    let job = RunJob {
        fingerprint: fingerprint(&character),
        stash_label: crate::stash_view::stash_label(stash_id),
        character,
        stash_id,
        options,
        plan: planned,
        steps,
        config,
        learning_enabled,
    };
    runner::spawn(&state.sorter, guard, job)
}

/// Stops the sort in progress, exactly as Ctrl+F12 does. `true` when one was running.
#[tauri::command]
pub fn sorter_stop(state: State<'_, AppState>) -> bool {
    state.input_lock.holder() == Some(SORTER) && state.input_lock.stop()
}

/// The current or last run; `busyWith` names another feature that drives the mouse right now.
#[tauri::command]
pub fn sorter_status(state: State<'_, AppState>) -> SorterStatus {
    let mut status = lock_status(&state.sorter.shared.status).clone();
    status.busy_with = state.input_lock.holder().filter(|&holder| holder != SORTER).map(str::to_string);
    status
}

/// The player's verdict on a finished sort, for learning. `false` when learning is off or the
/// session is unknown.
#[tauri::command]
pub fn sorter_feedback(state: State<'_, AppState>, session_id: String, success: bool, note: Option<String>) -> Result<bool, String> {
    let sorter: &Sorter = &state.sorter;
    let Some(learning) = sorter.learning.as_ref() else { return Ok(false) };
    let note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let accepted = learning.record_feedback(&session_id, success, note.as_deref());
    if accepted {
        let mut status = lock_status(&sorter.shared.status);
        if status.session_id.as_deref() == Some(session_id.as_str()) {
            status.feedback_sent = true;
        }
    }
    Ok(accepted)
}

/// Seconds since the game last sent this character (its saved file's age).
fn data_age_s(state: &AppState, character_id: &str) -> Option<u64> {
    if character_id.is_empty() || !character_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return None;
    }
    let file = format!("{character_id}.json");
    let own = crate::network::characters_folder(state).join(&file);
    let path = if own.exists() { own } else { crate::stash_view::dndtools_characters_folder()?.join(&file) };
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(modified.elapsed().map_or(0, |age| age.as_secs()))
}
