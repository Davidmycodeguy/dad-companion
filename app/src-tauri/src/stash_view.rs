//! The Stash page: the player's characters, each stash tab laid out like the game's grid, and what
//! every item is worth. Characters come from the files DnDTools saved (and, once live capture is
//! wired, from the game directly).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;

use market::card::{HoverCard, TipFacts};
use market::Confidence;
use state::stash::{BAG, EQUIPMENT, LAST_PURCHASED_STORAGE, STORAGE};
use state::{grid_size, is_off_limits, slot_cell, Character, OwnedItem};

use crate::hover::facts;
use crate::state::AppState;

/// Loads the characters DnDTools saved (one `<character id>.json` each in its data folder).
pub fn load_saved_characters(folder: &Path) -> Vec<Character> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut characters: Vec<Character> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_character_file(path))
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let json: serde_json::Value = serde_json::from_str(&text).ok()?;
            state::import::character_from_saved(&json)
                .inspect_err(|err| log::warn!("{} is not a character file: {err}", path.display()))
                .ok()
        })
        .collect();
    characters.sort_by(|a, b| a.name.cmp(&b.name));
    characters
}

/// Characters the app saved from the game itself (its own JSON), replacing DnDTools' copies of the
/// same characters: they are fresher.
pub fn merge_own_characters(mut characters: Vec<Character>, own_folder: &Path) -> Vec<Character> {
    let Ok(entries) = std::fs::read_dir(own_folder) else { return characters };
    for path in entries.flatten().map(|entry| entry.path()).filter(|path| is_character_file(path)) {
        let own: Option<Character> = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok());
        match own {
            Some(own) => match characters.iter_mut().find(|c| c.id == own.id) {
                Some(existing) => *existing = own,
                None => characters.push(own),
            },
            None => log::warn!("{} is not a saved character", path.display()),
        }
    }
    characters.sort_by(|a, b| a.name.cmp(&b.name));
    characters
}

/// Character files are named by the character's numeric id (other JSON files live there too).
fn is_character_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "json") && path.file_stem().and_then(|s| s.to_str()).is_some_and(is_character_id)
}

/// Longest character id accepted as a file name.
const MAX_CHARACTER_ID_LEN: usize = 32;

/// Whether `id` is safe to use as a file name: the game's character ids are numbers. Ids come from
/// the game's traffic, so anything else (a path, say) is refused rather than written to disk.
pub fn is_character_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_CHARACTER_ID_LEN && id.bytes().all(|b| b.is_ascii_digit())
}

/// Where DnDTools keeps its saved characters.
pub fn dndtools_characters_folder() -> Option<PathBuf> {
    appdata::DataDir::dndtools_root().map(|root| root.join("data").join("characters"))
}

/// The name the game shows on a stash tab.
pub(crate) fn stash_label(inventory_id: u32) -> String {
    match inventory_id {
        BAG => "Bag".into(),
        EQUIPMENT => "Equipped".into(),
        STORAGE => "Stash".into(),
        id @ 5..=LAST_PURCHASED_STORAGE => format!("Stash {}", id - STORAGE + 1),
        20 => "Seasonal".into(),
        21 => "Seasonal 2".into(),
        state::LOCKED_SEASONAL_STASH => "Shared (locked)".into(),
        id => format!("Stash {id}"),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashSummary {
    id: u32,
    label: String,
    items: usize,
    /// The locked Seasonal Shared Stash: its items are a preview, not the player's.
    locked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSummary {
    id: String,
    name: String,
    class: String,
    level: u32,
    stashes: Vec<StashSummary>,
}

#[tauri::command]
pub fn characters(state: State<'_, AppState>) -> Vec<CharacterSummary> {
    let characters = state.characters.lock().map(|c| c.clone()).unwrap_or_default();
    characters
        .iter()
        .map(|c| CharacterSummary {
            id: c.id.clone(),
            name: c.name.clone(),
            class: c.class.clone(),
            level: c.level,
            stashes: c
                .stash_ids()
                .into_iter()
                .map(|id| StashSummary {
                    id,
                    label: stash_label(id),
                    items: c.items.iter().filter(|i| i.inventory_id == id).count(),
                    locked: is_off_limits(id),
                })
                .collect(),
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashItemView {
    unique_id: String,
    item_id: String,
    name: String,
    rarity: &'static str,
    icon: Option<String>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    count: u32,
    /// Gold this item is (coins, or what a coin purse, pouch or bag holds); None otherwise.
    gold: Option<u64>,
    /// What the price model expects this exact item to sell for; None without market data (and
    /// for gold, which is worth its face value).
    value: Option<f64>,
    /// What a merchant pays for the whole stack.
    merchant: i64,
    tradable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashView {
    label: String,
    /// Grid size in cells; None for stashes without a grid (equipment).
    grid: Option<(u32, u32)>,
    locked: bool,
    items: Vec<StashItemView>,
    /// Gold in this stash: coins and what coin containers hold.
    total_gold: u64,
    /// What the other items are worth on the market.
    total_value: f64,
    total_merchant: i64,
}

#[tauri::command]
pub fn stash_view(state: State<'_, AppState>, character_id: String, inventory_id: u32) -> Result<StashView, String> {
    let character = find_character(&state, &character_id)?;
    let locked = is_off_limits(inventory_id);
    let grid = grid_size(inventory_id);
    let items: Vec<StashItemView> = character
        .stash(inventory_id)
        .iter()
        .enumerate()
        .map(|(index, item)| item_view(&state, item, grid, index, locked))
        .collect();
    Ok(StashView {
        label: stash_label(inventory_id),
        grid,
        locked,
        total_gold: items.iter().filter_map(|i| i.gold).sum(),
        total_value: items.iter().filter_map(|i| i.value).sum(),
        total_merchant: items.iter().map(|i| i.merchant).sum(),
        items,
    })
}

fn item_view(state: &AppState, item: &OwnedItem, grid: Option<(u32, u32)>, index: usize, locked: bool) -> StashItemView {
    let catalog = state.catalog.get(&item.item_id);
    let (width, height) = catalog.map_or((1, 1), |c| (c.width.max(1), c.height.max(1)));
    // Stashes without a grid (equipment) are drawn as a row of slots.
    let (x, y) = match grid {
        Some((grid_width, _)) => slot_cell(item.slot_id.unwrap_or(0), grid_width),
        None => (u32::try_from(index).unwrap_or(0) * 2, 0),
    };
    StashItemView {
        unique_id: item.unique_id.to_string(),
        item_id: item.item_id.clone(),
        name: catalog.map_or_else(|| item.item_id.clone(), |c| c.name.clone()),
        rarity: catalog.map_or("Unknown", |c| c.rarity.name()),
        icon: catalog.and_then(|c| c.icon_path.clone()),
        x,
        y,
        width,
        height,
        count: item.count,
        // Preview items of the locked stash are not the player's: no gold, no value.
        gold: if locked { None } else { item.gold() },
        value: if locked || item.gold().is_some() { None } else { item_value(state, item) },
        merchant: if locked || item.gold().is_some() { 0 } else { catalog.map_or(0, |c| i64::from(c.vendor_price) * i64::from(item.count)) },
        tradable: item.tradable,
    }
}

fn item_value(state: &AppState, item: &OwnedItem) -> Option<f64> {
    let model = state.worth()?;
    let rolls: Vec<(String, f64)> = item.rolls.iter().map(|(stat, value)| (stat.clone(), *value as f64)).collect();
    let slot = state.catalog.get(&item.item_id).map(|c| c.slot_type.as_str()).filter(|s| !s.is_empty());
    let estimate = model.predict(&item.item_id, &rolls, item.count.max(1), slot, None);
    (estimate.confidence != Confidence::Unknown).then_some(estimate.value)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterWealth {
    id: String,
    name: String,
    class: String,
    level: u32,
    gold: u64,
    /// What the price model expects the items to sell for.
    value: f64,
    /// What merchants pay for them.
    merchant: i64,
    items: usize,
}

/// What each character owns across every stash but the locked seasonal one.
#[tauri::command]
pub fn wealth(state: State<'_, AppState>) -> Vec<CharacterWealth> {
    let characters = state.characters.lock().map(|c| c.clone()).unwrap_or_default();
    characters.iter().map(|character| character_wealth(&state, character)).collect()
}

fn character_wealth(state: &AppState, character: &Character) -> CharacterWealth {
    let owned: Vec<&OwnedItem> = character.items.iter().filter(|i| !is_off_limits(i.inventory_id)).collect();
    let goods: Vec<&OwnedItem> = owned.iter().copied().filter(|i| i.gold().is_none()).collect();
    CharacterWealth {
        id: character.id.clone(),
        name: character.name.clone(),
        class: character.class.clone(),
        level: character.level,
        gold: owned.iter().filter_map(|i| i.gold()).sum(),
        value: goods.iter().filter_map(|i| item_value(state, i)).sum(),
        merchant: goods
            .iter()
            .map(|i| state.catalog.get(&i.item_id).map_or(0, |c| i64::from(c.vendor_price) * i64::from(i.count)))
            .sum(),
        items: goods.len(),
    }
}

/// The value card for one owned item, as if hovered in game.
#[tauri::command]
pub fn stash_item_card(state: State<'_, AppState>, character_id: String, unique_id: String) -> Result<HoverCard, String> {
    let character = find_character(&state, &character_id)?;
    let item = character.items.iter().find(|i| i.unique_id.to_string() == unique_id).ok_or("item not found")?;
    if is_off_limits(item.inventory_id) {
        return Err("Items in the locked seasonal stash are a preview, not yours.".into());
    }
    let catalog = state.catalog.get(&item.item_id);
    let tip = TipFacts {
        title: catalog.map_or_else(|| item.item_id.clone(), |c| c.name.clone()),
        rarity: catalog.map_or("Unknown", |c| c.rarity.name()).to_owned(),
        rolls: item.rolls.clone(),
        unread: 0,
    };
    Ok(facts::card_for(&state, &item.item_id, &tip, facts::now_s()))
}

fn find_character(state: &AppState, character_id: &str) -> Result<Character, String> {
    let characters = state.characters.lock().map_err(|_| "characters unavailable")?;
    characters.iter().find(|c| c.id == character_id).cloned().ok_or_else(|| format!("no character {character_id}"))
}

#[cfg(test)]
mod tests {
    use super::{is_character_file, stash_label};
    use std::path::Path;

    #[test]
    fn only_numeric_json_files_are_characters() {
        assert!(is_character_file(Path::new("10000001.json")));
        assert!(!is_character_file(Path::new("worth_model.json")));
        assert!(!is_character_file(Path::new("10000001.txt")));
    }

    #[test]
    fn only_numeric_ids_become_file_names() {
        assert!(super::is_character_id("10000001"));
        assert!(!super::is_character_id(""));
        assert!(!super::is_character_id("C:\\evil\\path"));
        assert!(!super::is_character_id("../../x"));
        assert!(!super::is_character_id(&"9".repeat(33)));
    }

    #[test]
    fn stash_tabs_have_the_games_names() {
        assert_eq!(stash_label(2), "Bag");
        assert_eq!(stash_label(4), "Stash");
        assert_eq!(stash_label(5), "Stash 2");
        assert_eq!(stash_label(30), "Shared (locked)");
    }
}
