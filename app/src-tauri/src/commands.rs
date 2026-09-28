//! Commands the UI calls.

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::State;

use game_data::Item;

use crate::product;
use crate::state::AppState;

const MAX_SEARCH_RESULTS: usize = 100;
const MAX_KEY_LEN: usize = 64;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    name: &'static str,
    version: &'static str,
    repo: &'static str,
    data_dir: String,
    items: usize,
    icons: usize,
    game_running: bool,
}

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppStatus {
    AppStatus {
        name: product::NAME,
        version: env!("CARGO_PKG_VERSION"),
        repo: product::GITHUB_REPO,
        data_dir: state.data.root().display().to_string(),
        items: state.catalog.len(),
        icons: state.icons.as_ref().map_or(0, |icons| icons.len()),
        game_running: crate::game::is_running(product::GAME_PROCESS),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Map<String, Value>, String> {
    let settings = state.settings.lock().map_err(|_| "settings are unavailable".to_owned())?;
    Ok(settings.all().clone())
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: Value) -> Result<(), String> {
    if !is_valid_key(&key) {
        return Err(format!("not a setting name: {key:?}"));
    }
    let mut settings = state.settings.lock().map_err(|_| "settings are unavailable".to_owned())?;
    settings.set(&key, value).map_err(|err| format!("could not save settings: {err}"))
}

/// Setting names are short camelCase identifiers (letters, digits, `.` and `_`).
fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// One item as the UI shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    id: String,
    name: String,
    rarity: &'static str,
    kind: String,
    item_type: String,
    width: u32,
    height: u32,
    max_stack: u32,
    vendor_price: u32,
    tradable: bool,
    icon: Option<String>,
}

impl From<&Item> for ItemView {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id.clone(),
            name: item.name.clone(),
            rarity: item.rarity.name(),
            kind: item.kind_line(),
            item_type: item.item_type.clone(),
            width: item.width.max(1),
            height: item.height.max(1),
            max_stack: item.max_stack,
            vendor_price: item.vendor_price,
            tradable: item.tradable,
            icon: item.icon_path.clone(),
        }
    }
}

/// An item name with its rarity variants, lowest rarity first.
#[derive(Serialize)]
pub struct ItemGroup {
    name: String,
    variants: Vec<ItemView>,
}

#[tauri::command]
pub fn search_items(state: State<'_, AppState>, query: String, limit: Option<usize>) -> Vec<ItemGroup> {
    let limit = limit.unwrap_or(30).min(MAX_SEARCH_RESULTS);
    state
        .catalog
        .search(&query, limit)
        .into_iter()
        .map(|hit| ItemGroup {
            name: hit.name.to_owned(),
            variants: hit.variants.into_iter().map(ItemView::from).collect(),
        })
        .collect()
}

#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: String) -> Option<ItemView> {
    state.catalog.get(&id).map(ItemView::from)
}

#[cfg(test)]
mod tests {
    use super::is_valid_key;

    #[test]
    fn setting_names_are_short_identifiers() {
        assert!(is_valid_key("closeToTray"));
        assert!(is_valid_key("hover.cardScale"));
        assert!(!is_valid_key(""));
        assert!(!is_valid_key("../evil"));
        assert!(!is_valid_key("with space"));
        assert!(!is_valid_key(&"x".repeat(65)));
    }
}
