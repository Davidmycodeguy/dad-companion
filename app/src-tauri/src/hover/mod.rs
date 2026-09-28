//! Item values on hover: the card's data from the market history and the price model.

pub mod facts;
pub mod live;
pub mod looper;
pub mod overlay;

use serde::Serialize;
use tauri::State;

use market::card::{HoverCard, TipFacts};
use market::DAY_S;

use crate::state::AppState;

/// A card for any item as if it were hovered, with the rolls of one of its open listings (the
/// median-priced one), for the Hover values page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardPreview {
    card: HoverCard,
    /// The listing whose rolls the preview uses, if any.
    listing_price: Option<i64>,
}

/// Shows the preview card for `item_id` in the overlay beside a made-up tooltip box (physical
/// pixels), or hides it when `item_id` is empty. For trying the overlay without the game.
#[tauri::command]
pub fn demo_overlay(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    item_id: String,
    tooltip: overlay::Region,
) -> Result<(), String> {
    if item_id.is_empty() {
        overlay::hide(&app);
        return Ok(());
    }
    let preview = preview_card(state, item_id)?;
    let window = tauri::Manager::get_webview_window(&app, overlay::CARD_WINDOW).ok_or("card window missing")?;
    let screen = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .map(|m| (m.size().width as i32, m.size().height as i32))
        .unwrap_or((3840, 2160));
    overlay::show(&app, &preview.card, tooltip, screen);
    Ok(())
}

#[tauri::command]
pub fn preview_card(state: State<'_, AppState>, item_id: String) -> Result<CardPreview, String> {
    let item = state.catalog.get(&item_id).ok_or_else(|| format!("no item {item_id}"))?;
    let now = facts::now_s();
    let listing = state.market.as_ref().and_then(|market| {
        let open = market.open_listings(&item_id, now, 7.0 * DAY_S).ok()?;
        open.get(open.len() / 2).cloned()
    });
    let tip = TipFacts {
        title: item.name.clone(),
        rarity: item.rarity.name().to_owned(),
        rolls: listing.as_ref().map(|l| l.rolls.iter().map(|r| (r.id.clone(), r.value)).collect()).unwrap_or_default(),
        unread: 0,
    };
    Ok(CardPreview { card: facts::card_for(&state, &item_id, &tip, now), listing_price: listing.map(|l| l.price) })
}
