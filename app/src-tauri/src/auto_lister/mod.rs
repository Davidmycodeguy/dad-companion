//! The Auto lister page: its rules, the plan it builds from a character's stash, and what the game
//! last showed of the player's listings. Runs that click in the game live in [`run`].

mod plan_view;
mod pricing;
mod reprice;
pub mod run;
pub mod runtime;
mod stash;

use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use lister::marketplace_state::{MarketplaceState, MY_ITEM_SOLD};
use lister::plan::{build_plan, Plan};
use market::ListerRules;
use state::Character;

use crate::hover::facts;
use crate::state::AppState;
use plan_view::{ItemInfo, ListingsInfo, PlanResponse, PlanView};
use pricing::Pricing;

/// Where the rules are saved, in DnDTools' own format.
const RULES_KEY: &str = "listerRules";
/// Stash data older than this gets a warning: items may have moved since.
const STALE_DATA_S: f64 = 300.0;

fn load_rules(state: &AppState) -> ListerRules {
    let saved = state
        .settings
        .lock()
        .ok()
        .and_then(|s| s.get::<serde_json::Value>(RULES_KEY));
    saved.map_or_else(ListerRules::default, |value| ListerRules::from_dict(&value))
}

#[tauri::command]
pub fn lister_rules(state: State<'_, AppState>) -> ListerRules {
    load_rules(&state)
}

/// Saves the rules after clamping every value into range; the cleaned rules come back.
#[tauri::command]
pub fn save_lister_rules(
    state: State<'_, AppState>,
    rules: ListerRules,
) -> Result<ListerRules, String> {
    let clean = ListerRules::from_dict(&rules.to_dict());
    let mut settings = state.settings.lock().map_err(|_| "settings unavailable")?;
    settings
        .set(RULES_KEY, clean.to_dict())
        .map_err(|err| err.to_string())?;
    Ok(clean)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    id: String,
    label: String,
    items: usize,
}

/// The stash tabs a character's items can be listed from (never the locked seasonal stash).
#[tauri::command]
pub fn lister_sources(
    state: State<'_, AppState>,
    character_id: String,
) -> Result<Vec<SourceView>, String> {
    let character = find_character(&state, &character_id)?;
    Ok(stash::listable_stash_ids(&character)
        .into_iter()
        .map(|id| SourceView {
            id: id.to_string(),
            label: crate::stash_view::stash_label(id),
            items: character
                .items
                .iter()
                .filter(|i| i.inventory_id == id)
                .count(),
        })
        .collect())
}

/// Builds the plan: picks items by the rules, prices them from saved listings or the value
/// formula (or leaves them for a game search), and explains everything left out.
#[tauri::command]
pub fn lister_build_plan(
    state: State<'_, AppState>,
    character_id: String,
    rules: ListerRules,
) -> Result<PlanResponse, String> {
    let rules = ListerRules::from_dict(&rules.to_dict());
    let character = find_character(&state, &character_id)?;
    let stashes = stash::stashes_json(&character, &state.catalog, &rules.source_stash_ids);
    let snapshot = state.marketplace.snapshot();
    let free_spots = snapshot
        .as_ref()
        .map(|s| i64::try_from(s.free()).unwrap_or(i64::MAX));
    let data_age = character_data_age(&state, &character_id);
    let listed = state.marketplace.listed_ids();
    let tabs = stash::tab_mapping(&character);
    let build = |rules: &ListerRules, spots: Option<i64>| {
        build_plan(
            &stashes,
            rules,
            None,
            &tabs,
            spots,
            data_age,
            &|| {},
            &listed,
        )
        .map_err(|err| err.to_string())
    };

    let live = rules.price_source == "live";
    let mut plan = if live {
        build(&rules, free_spots)?
    } else {
        let pricing = pricing_for(&state);
        pricing.price_without_game(&rules, build(&pricing::uncapped(&rules), None)?, free_spots)
    };
    if rules.price_source == "database" {
        plan.warnings.extend(saved_data_warning(&state));
    }
    Ok(PlanResponse {
        items: item_infos(&state, &character, &plan),
        plan: PlanView::from(&plan),
        listings: listings_info(&state.marketplace),
        needs_game_pricing: live && !plan.entries.is_empty(),
    })
}

/// A hint when the saved listings are too old to count as the market: prices need fresh ones.
fn saved_data_warning(state: &AppState) -> Option<String> {
    let newest = state.market.as_ref()?.newest_seen().ok()??;
    let age = facts::now_s() - newest;
    (age > pricing::HISTORY_MAX_AGE_S).then(|| {
        format!(
            "The newest saved listing is {} hours old: press Update under Market data, or price from the value formula.",
            (age / 3600.0).floor()
        )
    })
}

/// What pricing reads right now: saved listings, the value model and our own listing ids.
fn pricing_for(state: &AppState) -> Pricing<'_> {
    let mut own = state.marketplace.own_listing_ids();
    if let Some(market) = state.market.as_ref() {
        match market.my_listing_ids() {
            Ok(ids) => own.extend(ids),
            Err(err) => log::warn!("own listings could not be read: {err}"),
        }
    }
    Pricing {
        market: state.market.as_ref(),
        worth: state.worth(),
        learned: state.learned(),
        own_listing_ids: own,
        now: facts::now_s(),
    }
}

/// Icons, names and rolls for every item the plan mentions, looked up in the character's stash.
fn item_infos(state: &AppState, character: &Character, plan: &Plan) -> HashMap<String, ItemInfo> {
    let wanted: std::collections::HashSet<&str> = plan
        .entries
        .iter()
        .map(|e| e.unique_id.as_str())
        .chain(plan.skipped.iter().map(|s| s.unique_id.as_str()))
        .collect();
    character
        .items
        .iter()
        .filter(|item| wanted.contains(item.unique_id.to_string().as_str()))
        .map(|item| {
            let meta = state.catalog.get(&item.item_id);
            let info = ItemInfo {
                icon: meta.and_then(|m| m.icon_path.clone()),
                rarity: meta.map_or("Unknown", |m| m.rarity.name()),
                stash_label: crate::stash_view::stash_label(item.inventory_id),
                rolls: item
                    .rolls
                    .iter()
                    .map(|(stat, value)| {
                        format!(
                            "{} {}",
                            game_data::roll_text(stat, *value),
                            game_data::stat_label(stat)
                        )
                    })
                    .collect(),
            };
            (item.unique_id.to_string(), info)
        })
        .collect()
}

/// Free spots and payouts from the last My Listings screen the game showed.
pub fn listings_info(marketplace: &MarketplaceState) -> ListingsInfo {
    let Some(snapshot) = marketplace.snapshot() else {
        return ListingsInfo::default();
    };
    let age = (marketplace.now() - snapshot.received_at).max(0.0);
    ListingsInfo {
        seen: true,
        free: Some(snapshot.free()),
        age_s: Some(age.round() as u64),
        payouts: snapshot.payouts.len(),
        payout_gold: snapshot
            .payouts
            .iter()
            .filter(|p| p.1 == MY_ITEM_SOLD)
            .map(|p| p.3)
            .sum(),
    }
}

/// Seconds since the character's stash was last read from the game (its saved file's age).
fn character_data_age(state: &AppState, character_id: &str) -> Option<f64> {
    if !crate::stash_view::is_character_id(character_id) {
        return None;
    }
    let file = format!("{character_id}.json");
    let own = crate::network::characters_folder(state).join(&file);
    let path = if own.exists() {
        own
    } else {
        crate::stash_view::dndtools_characters_folder()?.join(&file)
    };
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    Some(modified.elapsed().map_or(0.0, |d| d.as_secs_f64()))
}

/// Whether the stash data is old enough that items may have moved.
pub fn is_stale(age: Option<f64>) -> bool {
    age.is_some_and(|a| a > STALE_DATA_S)
}

fn find_character(state: &AppState, character_id: &str) -> Result<Character, String> {
    let characters = state
        .characters
        .lock()
        .map_err(|_| "characters unavailable")?;
    characters
        .iter()
        .find(|c| c.id == character_id)
        .cloned()
        .ok_or_else(|| "Pick a character first.".to_string())
}
