//! Live game data: the game's lobby traffic captured from Npcap, decoded, and turned into market
//! history and the player's stashes. Read-only: nothing is ever sent to the game or anywhere else.

use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use lister::marketplace_state::{
    ItemListInfo, ItemListMessage, MyItemInfo, MyItemListMessage, PropertyEntry, RegisterResMessage, TransferResMessage,
};
use lister::merchant_state::{QuestListMessage, SellBackMessage};
use market::history::{item_id_from_design, stat_from_property, MerchantOffer, MyListing, PageListing};
use protocol::messages::{proto, Decoded};
use protocol::{Decoder, DecoderEvent};
use state::{Character, OwnedItem};

use crate::hover::facts;
use crate::state::AppState;

/// How long a wait for the next segment lasts before checking whether to stop.
const RECV_TIMEOUT: Duration = Duration::from_millis(500);
/// The UI refreshes what these events name.
const GAME_DATA_EVENT: &str = "game-data";
const CLASS_PREFIX: &str = "Id_PlayerCharacter_";

/// Whether live game data is flowing, and why not, for the Overview and Settings pages.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    /// "starting", "on" or "off".
    pub state: &'static str,
    pub adapter: Option<String>,
    /// Why live data is off.
    pub error: Option<String>,
    /// Npcap is missing or too old: installing it turns live data on.
    pub needs_npcap: bool,
    /// When the last game message arrived (Unix seconds).
    pub last_message_at: Option<f64>,
}

impl Default for LiveStatus {
    fn default() -> Self {
        LiveStatus { state: "starting", adapter: None, error: None, needs_npcap: false, last_message_at: None }
    }
}

#[tauri::command]
pub fn live_status(state: State<'_, AppState>) -> LiveStatus {
    state.live.lock().map(|s| s.clone()).unwrap_or_default()
}

fn set_live<R: Runtime>(app: &AppHandle<R>, update: impl FnOnce(&mut LiveStatus)) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut live) = state.live.lock() {
            update(&mut live);
        }
    }
    let _ = app.emit(GAME_DATA_EVENT, "live");
}

/// Starts capturing the game's traffic on its own thread for the app's lifetime. Without Npcap (or
/// a usable network adapter) live data stays off, and the status says why.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("game-traffic".into()).spawn(move || {
        let capture = match capture::Capture::start(capture::Config::default()) {
            Ok(capture) => capture,
            Err(err) => {
                log::warn!("live game data is off: {err}");
                let needs_npcap = matches!(err, capture::Error::NpcapNotFound(_) | capture::Error::MissingSymbol(_));
                set_live(&app, |live| {
                    *live = LiveStatus { state: "off", error: Some(err.to_string()), needs_npcap, ..LiveStatus::default() };
                });
                return;
            }
        };
        log::info!("reading the game's traffic on {}", capture.adapter_name());
        let adapter = capture.adapter_name().to_owned();
        set_live(&app, |live| *live = LiveStatus { state: "on", adapter: Some(adapter), ..LiveStatus::default() });
        let mut decoder = Decoder::new();
        loop {
            match capture.recv_timeout(RECV_TIMEOUT) {
                Ok(segment) => {
                    for event in decoder.feed(&segment) {
                        handle(&app, event);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let reason = format!("{:?}", capture.fatal_error());
                    log::warn!("the game's traffic stopped: {reason}");
                    set_live(&app, |live| {
                        live.state = "off";
                        live.error = Some(format!("Reading the game's traffic stopped: {reason}"));
                    });
                    break;
                }
            }
        }
    });
    if let Err(err) = spawned {
        log::error!("live game data could not start: {err}");
    }
}

fn handle<R: Runtime>(app: &AppHandle<R>, event: DecoderEvent) {
    match event {
        DecoderEvent::Message(message) => {
            log::debug!("game message {}", message.name);
            dispatch(app, message.decoded);
        }
        DecoderEvent::DecodeFailed { packet_type, error } => log::debug!("game message {packet_type} not decoded: {error}"),
        DecoderEvent::Desync { dropped, reason, .. } => log::debug!("game traffic desync, {dropped} bytes dropped: {reason}"),
    }
}

fn dispatch<R: Runtime>(app: &AppHandle<R>, decoded: Decoded) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let now = facts::now_s();
    if let Ok(mut live) = state.live.lock() {
        live.last_message_at = Some(now);
    }
    if state.quests.as_ref().is_some_and(|quests| quests.handle(&decoded)) {
        let _ = app.emit(GAME_DATA_EVENT, "quests");
    }
    let recorded = match decoded {
        Decoded::MarketplaceItemList(page) => {
            state.marketplace.handle_item_list(&item_list_message(&page));
            state.market.as_ref().map(|market| {
                let rows: Vec<PageListing> = page.item_infos.iter().filter_map(page_listing).collect();
                market.record_listings(&rows, now).map(|_| "market")
            })
        }
        Decoded::MarketplaceMyItemList(mine) => {
            state.marketplace.handle_my_item_list(&my_item_list_message(&mine));
            let _ = app.emit(GAME_DATA_EVENT, "listings");
            state.market.as_ref().map(|market| {
                let rows: Vec<MyListing> = mine.my_item_infos.iter().filter_map(my_listing).collect();
                market.record_my_listings(&rows, now).map(|()| "market")
            })
        }
        Decoded::MerchantStock(stock) => state.market.as_ref().map(|market| {
            let offers: Vec<MerchantOffer> = stock.stock_list.iter().filter_map(merchant_offer).collect();
            market.record_merchant_stock(&offers, now).map(|_| "market")
        }),
        Decoded::CharacterInfo(info) => {
            if let Some(character) = info.character_data_base.as_ref().map(character) {
                save_character(&state, &character);
                upsert_character(&state, character);
                let _ = app.emit(GAME_DATA_EVENT, "characters");
            }
            None
        }
        Decoded::MarketplaceRegister(answer) => {
            state.marketplace.handle_register_res(RegisterResMessage { result: i64::from(answer.result) });
            None
        }
        Decoded::MarketplaceTransfer(answer) => {
            state.marketplace.handle_transfer_res(TransferResMessage { result: i64::from(answer.result) });
            None
        }
        Decoded::MarketplaceItemSold(_) => Some(Ok("sold")),
        Decoded::MerchantQuestList(list) => {
            let quest_ids = list.quests.iter().map(|quest| quest.quest_id.clone()).collect();
            state.merchant.handle_quest_list(&QuestListMessage { quest_ids });
            None
        }
        Decoded::MerchantSellBack(answer) => {
            let delete_unique_ids = answer.merchant_result.as_ref().map(|r| r.delete_unique_ids.clone()).unwrap_or_default();
            state.merchant.handle_sell_back(&SellBackMessage { result: i64::from(answer.result), delete_unique_ids });
            None
        }
        // Listed one by one (no catch-all) so a new message type is a compile error until it is
        // either handled here or deliberately ignored. Merchant sales and quests are wired with
        // their features; inventory info is covered by the character info that follows it.
        Decoded::MerchantList(_)
        | Decoded::MerchantQuestLogList(_)
        | Decoded::MerchantQuestSelect(_)
        | Decoded::MerchantQuestComplete(_)
        | Decoded::MerchantQuestContentValueStack(_)
        | Decoded::InventoryInfo(_)
        | Decoded::Other { .. } => None,
    };
    match recorded {
        Some(Ok(kind)) => {
            let _ = app.emit(GAME_DATA_EVENT, kind);
        }
        Some(Err(err)) => log::warn!("could not record game data: {err}"),
        None => {}
    }
}

fn stats(properties: &[proto::SItemProperty]) -> Vec<(String, i64)> {
    properties.iter().map(|p| (stat_from_property(&p.property_type_id), i64::from(p.property_value))).collect()
}

fn market_stats(properties: &[proto::SItemProperty]) -> Vec<market::Stat> {
    stats(properties).into_iter().map(|(id, value)| market::Stat { id, value }).collect()
}

fn page_listing(info: &proto::SmarketplaceItemInfo) -> Option<PageListing> {
    let item = info.item.as_ref()?;
    Some(PageListing {
        listing_id: info.listing_id.to_string(),
        item_id: item_id_from_design(&item.item_id),
        price: i64::from(info.price),
        count: i64::from(item.item_count.max(1)),
        base: market_stats(&item.primary_property_array),
        rolls: market_stats(&item.secondary_property_array),
        seller: info.nickname.as_ref().map(|n| n.original_nick_name.clone()).unwrap_or_default(),
        remain_ms: info.remain_expiration_time,
    })
}

fn my_listing(info: &proto::SmarketplaceMyItemInfo) -> Option<MyListing> {
    let listing = info.item_info.as_ref()?;
    Some(MyListing {
        listing_id: listing.listing_id.to_string(),
        item_id: listing.item.as_ref().map(|i| item_id_from_design(&i.item_id)).unwrap_or_default(),
        price: i64::from(listing.price),
        state: info.my_item_state,
    })
}

/// My Listings as the lister tracks it: free spots, payouts, and which items are up for sale.
fn my_item_list_message(mine: &proto::Ss2cMarketplaceMyItemListRes) -> MyItemListMessage {
    MyItemListMessage {
        available_order_indexes: mine.available_order_indexes.iter().map(|&i| i64::from(i)).collect(),
        current_page: i64::from(mine.current_page),
        my_item_infos: mine
            .my_item_infos
            .iter()
            .map(|info| {
                let listing = info.item_info.as_ref();
                let item = listing.and_then(|l| l.item.as_ref());
                MyItemInfo {
                    order_index: i64::from(info.order_index),
                    my_item_state: i64::from(info.my_item_state),
                    item_unique_id: item.map_or(0, |i| i.item_unique_id),
                    item_id: item.map(|i| i.item_id.clone()).unwrap_or_default(),
                    price: listing.map_or(0, |l| i64::from(l.price)),
                    listing_id: listing.map_or(0, |l| u64::try_from(l.listing_id).unwrap_or(0)),
                }
            })
            .collect(),
    }
}

/// A search results page as the lister reads it while pricing.
fn item_list_message(page: &proto::Ss2cMarketplaceItemListRes) -> ItemListMessage {
    let properties = |list: &[proto::SItemProperty]| -> Vec<PropertyEntry> {
        list.iter()
            .map(|p| PropertyEntry { property_type_id: p.property_type_id.clone(), property_value: i64::from(p.property_value) })
            .collect()
    };
    ItemListMessage {
        item_infos: page
            .item_infos
            .iter()
            .filter_map(|info| {
                let item = info.item.as_ref()?;
                Some(ItemListInfo {
                    item_id: item.item_id.clone(),
                    price: i64::from(info.price),
                    item_count: i64::from(item.item_count),
                    listing_id: info.listing_id,
                    primary_properties: properties(&item.primary_property_array),
                    secondary_properties: properties(&item.secondary_property_array),
                })
            })
            .collect(),
        current_page: i64::from(page.current_page),
        max_page: i64::from(page.max_page),
    }
}

fn merchant_offer(stock: &proto::SmerchantStockBuyItemInfo) -> Option<MerchantOffer> {
    let item = stock.item_info.as_ref()?;
    Some(MerchantOffer {
        item_id: item_id_from_design(&item.item_id),
        count: i64::from(item.item_count.max(1)),
        final_price: i64::from(stock.final_price),
    })
}

fn owned_item(item: &proto::SItem, storage_id: Option<u32>) -> OwnedItem {
    OwnedItem {
        unique_id: item.item_unique_id,
        item_id: item_id_from_design(&item.item_id),
        count: item.item_count.max(1),
        contents: item.item_contents_count,
        inventory_id: if item.inventory_id != 0 { item.inventory_id } else { storage_id.unwrap_or(0) },
        slot_id: Some(item.slot_id),
        base: stats(&item.primary_property_array),
        rolls: stats(&item.secondary_property_array),
        loot_state: item.loot_state,
        tradable: item.tradable == 1,
    }
}

fn character(info: &proto::ScharacterInfo) -> Character {
    let mut items: Vec<OwnedItem> = info.character_item_list.iter().map(|item| owned_item(item, None)).collect();
    for storage in &info.character_storage_infos {
        items.extend(storage.character_storage_item_list.iter().map(|item| owned_item(item, Some(storage.inventory_id))));
    }
    Character {
        id: info.character_id.clone(),
        name: info.nick_name.as_ref().map(|n| n.original_nick_name.clone()).unwrap_or_default(),
        class: info.character_class.rsplit(CLASS_PREFIX).next().unwrap_or_default().to_owned(),
        level: info.level,
        items,
        storages: info.character_storage_infos.iter().map(|s| s.inventory_id).collect(),
    }
}

fn upsert_character(state: &AppState, character: Character) {
    if let Ok(mut characters) = state.characters.lock() {
        match characters.iter_mut().find(|c| c.id == character.id) {
            Some(existing) => *existing = character,
            None => characters.push(character),
        }
    }
}

/// Where the app keeps the characters it read from the game.
pub fn characters_folder(state: &AppState) -> PathBuf {
    state.data.data().join("characters")
}

/// Saves a character read from the game (written whole, then renamed, so a crash never leaves half a file).
fn save_character(state: &AppState, character: &Character) {
    if !crate::stash_view::is_character_id(&character.id) {
        log::warn!("not saving a character with an unexpected id ({} characters)", character.id.len());
        return;
    }
    let folder = characters_folder(state);
    let result = std::fs::create_dir_all(&folder).and_then(|()| {
        let json = serde_json::to_vec_pretty(character).map_err(std::io::Error::other)?;
        let path = folder.join(format!("{}.json", character.id));
        let partial = path.with_extension("json.partial");
        std::fs::write(&partial, json)?;
        std::fs::rename(partial, path)
    });
    if let Err(err) = result {
        log::warn!("could not save character {}: {err}", character.id);
    }
}
