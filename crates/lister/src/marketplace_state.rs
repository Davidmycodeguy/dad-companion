//! Tracks Marketplace packets so the lister can confirm each listing.
//!
//! Port of DnDTools' `src/models/marketplace_state.py`. The message types below are plain mirrors
//! of the fields the original Python handlers read off the decoded protobuf messages — converting
//! a real network message into one is the caller's job (the protocol crate is out of scope here).

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use market::MarketRow;

use crate::clock::{MonotonicClock, SharedClock};

/// `SS2C_MARKETPLACE_ITEM_REGISTER_RES.result` for a listing that went through.
pub const REGISTER_SUCCESS: i64 = 1;
/// A `snapshot()` older than this is too stale for a caller to act on (used by the runner, not
/// enforced here).
pub const MAX_SNAPSHOT_AGE_S: f64 = 120.0;
/// `currentPage` base (0 or 1) unverified — 0 never mistakes page 2 for page 1; flip to 1 if the
/// in-game dry run refuses on page 1.
pub const FIRST_PAGE: i64 = 0;
/// Fail codes that mean the specific item can't be listed at all (not a transient failure).
pub const ITEM_LEVEL_FAIL_CODES: [i64; 2] = [662, 666];
const ITEM_ID_PREFIX: &str = "Id_Item_";
/// `myItemState` values (3 = sold, verified in game).
pub const MY_ITEM_LISTING: i64 = 1;
pub const MY_ITEM_EXPIRED: i64 = 2;
pub const MY_ITEM_SOLD: i64 = 3;
const PAYOUT_STATES: [i64; 2] = [MY_ITEM_EXPIRED, MY_ITEM_SOLD];

/// One `myItemInfos` entry of `S2C_MARKETPLACE_MY_ITEM_LIST_RES`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyItemInfo {
    pub order_index: i64,
    pub my_item_state: i64,
    pub item_unique_id: u64,
    /// Raw `itemId`, e.g. `"DesignDataItem:Id_Item_GreatHelm_3001"`.
    pub item_id: String,
    pub price: i64,
    pub listing_id: u64,
}

/// `S2C_MARKETPLACE_MY_ITEM_LIST_RES`: one snapshot of the player's Marketplace listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyItemListMessage {
    pub available_order_indexes: Vec<i64>,
    pub current_page: i64,
    pub my_item_infos: Vec<MyItemInfo>,
}

/// One `propertyType, value` pair off an item's primary/secondary property array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyEntry {
    pub property_type_id: String,
    pub property_value: i64,
}

/// One `itemInfos` entry of `S2C_MARKETPLACE_ITEM_LIST_RES`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemListInfo {
    pub item_id: String,
    pub price: i64,
    pub item_count: i64,
    pub listing_id: i64,
    pub primary_properties: Vec<PropertyEntry>,
    pub secondary_properties: Vec<PropertyEntry>,
}

/// `S2C_MARKETPLACE_ITEM_LIST_RES`: one page of View Market search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemListMessage {
    pub item_infos: Vec<ItemListInfo>,
    pub current_page: i64,
    pub max_page: i64,
}

/// `S2C_MARKETPLACE_TRANSFER_ITEMS_RES` after "Transfer All Items" on a sold / expired listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferResMessage {
    pub result: i64,
}

/// `S2C_MARKETPLACE_ITEM_REGISTER_RES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterResMessage {
    pub result: i64,
}

fn stats(properties: &[PropertyEntry]) -> Vec<(String, i64)> {
    properties.iter().map(|p| (market::stat_name(&p.property_type_id).to_string(), p.property_value)).collect()
}

fn market_row(info: &ItemListInfo) -> MarketRow {
    MarketRow {
        item_id: info.item_id.rsplit(ITEM_ID_PREFIX).next().unwrap_or(&info.item_id).to_string(),
        price: info.price,
        base: stats(&info.primary_properties),
        rolls: stats(&info.secondary_properties),
        listing_id: info.listing_id.to_string(),
        count: info.item_count.max(1),
    }
}

/// A human message for a Marketplace failure code (`describe_fail_code` in the Python).
pub fn describe_fail_code(code: i64) -> String {
    let message = match code {
        650 => "Marketplace general error",
        655 => "Maximum number of listings reached",
        656 => "Price was not set (typing may have failed)",
        653 => "Not enough inventory space",
        657 => "Not enough gold for the listing fee",
        658 => "Not enough space in your stash/inventory to take the item back",
        660 => "Price is above the maximum allowed",
        662 => "Item was looted in a raid and can't be traded",
        663 => "Squires can't list items",
        664 => "Not enough play time to list items",
        665 => "Can't list while matchmaking",
        666 => "Item is not tradable",
        _ => return format!("Marketplace error {code}"),
    };
    message.to_string()
}

/// One My Listings snapshot: free spots, the page it came from, and any payouts waiting to be
/// collected.
#[derive(Debug, Clone, PartialEq)]
pub struct ListingsSnapshot {
    pub received_at: f64,
    /// Free spot order indexes (`availableOrderIndexes`).
    pub available: Vec<i64>,
    pub current_page: i64,
    /// `(order_index, my_item_state, item_id, price)` for sold / expired listings awaiting transfer.
    pub payouts: Vec<(i64, i64, String, i64)>,
}

impl ListingsSnapshot {
    pub fn free(&self) -> usize {
        self.available.len()
    }
}

/// The result of `wait_for_register`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    Ok,
    Failed(i64),
    /// No `SS2C_MARKETPLACE_ITEM_REGISTER_RES` arrived within the timeout.
    Timeout,
}

#[derive(Default)]
struct Inner {
    snapshot: Option<ListingsSnapshot>,
    /// `itemUniqueId` (stringified) -> last `received_at` seen for it.
    listed_at: HashMap<String, f64>,
    /// `itemUniqueId` (stringified) -> latest `myItemState`.
    listing_state: HashMap<String, i64>,
    /// `listingId` (stringified) -> latest `myItemState`, for excluding our own asks.
    own_listings: HashMap<String, i64>,
    register_result: Option<i64>,
    item_list: Option<(f64, Vec<MarketRow>, i64, i64)>,
    transfer_result: Option<i64>,
}

/// Tracks Marketplace packets across threads: the packet reader thread calls the `handle_*`
/// methods as messages arrive, while the lister thread calls the `wait_for_*` methods to block
/// (with a timeout) until the packet it needs shows up.
pub struct MarketplaceState {
    clock: SharedClock,
    inner: Mutex<Inner>,
    cond: Condvar,
}

impl Default for MarketplaceState {
    fn default() -> Self {
        Self::new(MonotonicClock::shared())
    }
}

impl MarketplaceState {
    pub fn new(clock: SharedClock) -> Self {
        MarketplaceState { clock, inner: Mutex::new(Inner::default()), cond: Condvar::new() }
    }

    pub fn now(&self) -> f64 {
        self.clock.now()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("MarketplaceState mutex is never held across a panic")
    }

    pub fn handle_my_item_list(&self, message: &MyItemListMessage) {
        let received = self.clock.now();
        let mut inner = self.lock();
        let payouts = message
            .my_item_infos
            .iter()
            .filter(|info| PAYOUT_STATES.contains(&info.my_item_state))
            .map(|info| {
                let bare_id = info.item_id.rsplit(ITEM_ID_PREFIX).next().unwrap_or(&info.item_id).to_string();
                (info.order_index, info.my_item_state, bare_id, info.price)
            })
            .collect();
        inner.snapshot = Some(ListingsSnapshot {
            received_at: received,
            available: message.available_order_indexes.clone(),
            current_page: message.current_page,
            payouts,
        });
        for info in &message.my_item_infos {
            let key = info.item_unique_id.to_string();
            let state = if info.my_item_state == 0 { MY_ITEM_LISTING } else { info.my_item_state };
            inner.listed_at.insert(key, received);
            inner.listing_state.insert(info.item_unique_id.to_string(), state);
            inner.own_listings.insert(info.listing_id.to_string(), state);
        }
        self.cond.notify_all();
    }

    /// `S2C_MARKETPLACE_ITEM_LIST_RES`: one page of View Market search results.
    pub fn handle_item_list(&self, message: &ItemListMessage) {
        let received = self.clock.now();
        let rows: Vec<MarketRow> = message.item_infos.iter().map(market_row).collect();
        let mut inner = self.lock();
        inner.item_list = Some((received, rows, message.current_page, message.max_page));
        self.cond.notify_all();
    }

    /// `(currentPage, maxPage)` of the latest search result page, or `None`.
    pub fn last_item_page(&self) -> Option<(i64, i64)> {
        self.lock().item_list.as_ref().map(|(_, _, page, max_page)| (*page, *max_page))
    }

    pub fn handle_transfer_res(&self, message: TransferResMessage) {
        let mut inner = self.lock();
        inner.transfer_result = Some(message.result);
        self.cond.notify_all();
    }

    pub fn begin_transfer(&self) {
        self.lock().transfer_result = None;
    }

    pub fn handle_register_res(&self, message: RegisterResMessage) {
        let mut inner = self.lock();
        inner.register_result = Some(message.result);
        self.cond.notify_all();
    }

    pub fn begin_register(&self) {
        self.lock().register_result = None;
    }

    pub fn snapshot(&self) -> Option<ListingsSnapshot> {
        self.lock().snapshot.clone()
    }

    /// Listing ids of our own active listings — never compare our prices against them.
    pub fn own_listing_ids(&self) -> std::collections::HashSet<String> {
        self.lock().own_listings.iter().filter(|(_, &state)| state == MY_ITEM_LISTING).map(|(k, _)| k.clone()).collect()
    }

    pub fn listed_ids(&self) -> std::collections::HashSet<String> {
        // Only items still up for sale; expired items come back to the stash and can be relisted.
        self.lock().listing_state.iter().filter(|(_, &state)| state == MY_ITEM_LISTING).map(|(k, _)| k.clone()).collect()
    }

    /// Blocks (up to `timeout` seconds) until a `handle_*` call satisfies `ready`, mirroring
    /// Python's `Condition.wait_for(pred, timeout)`. `timeout <= 0` still checks once before
    /// giving up, matching `Condvar::wait_timeout`'s own handling of a zero duration.
    fn wait_for<T: Clone>(&self, timeout: f64, mut extract: impl FnMut(&Inner) -> Option<T>) -> Option<T> {
        let guard = self.lock();
        if let Some(v) = extract(&guard) {
            return Some(v);
        }
        let (guard, _) = self
            .cond
            .wait_timeout_while(guard, Duration::from_secs_f64(timeout.max(0.0)), |inner| extract(inner).is_none())
            .expect("MarketplaceState mutex is never held across a panic");
        extract(&guard)
    }

    /// The MarketRows from the first search result page received after `since`.
    pub fn wait_for_item_list(&self, since: f64, timeout: f64) -> Option<Vec<MarketRow>> {
        self.wait_for(timeout, |inner| match &inner.item_list {
            Some((received_at, rows, ..)) if *received_at > since => Some(rows.clone()),
            _ => None,
        })
    }

    /// The transfer result code, or `None` if the game did not answer in time.
    pub fn wait_for_transfer(&self, timeout: f64) -> Option<i64> {
        self.wait_for(timeout, |inner| inner.transfer_result)
    }

    pub fn wait_for_register(&self, timeout: f64) -> RegisterOutcome {
        match self.wait_for(timeout, |inner| inner.register_result) {
            None => RegisterOutcome::Timeout,
            Some(REGISTER_SUCCESS) => RegisterOutcome::Ok,
            Some(other) => RegisterOutcome::Failed(other),
        }
    }

    pub fn wait_for_listing(&self, unique_id: &str, since: f64, timeout: f64) -> bool {
        self.wait_for(timeout, |inner| (inner.listed_at.get(unique_id).is_some_and(|&t| t > since)).then_some(())).is_some()
    }

    /// A My Listings snapshot received after `since`, or `None` if none arrived in time.
    pub fn wait_for_fresh_snapshot(&self, since: f64, timeout: f64) -> Option<ListingsSnapshot> {
        self.wait_for(timeout, |inner| match &inner.snapshot {
            Some(s) if s.received_at > since => Some(s.clone()),
            _ => None,
        })
    }

    /// The My Listings snapshot, waiting up to `timeout` for one received after `since` — but
    /// returning whatever snapshot is there (even a stale one) once the timeout elapses.
    pub fn wait_for_snapshot(&self, since: f64, timeout: f64) -> Option<ListingsSnapshot> {
        self.wait_for(timeout, |inner| match &inner.snapshot {
            Some(s) if s.received_at > since => Some(s.clone()),
            _ => None,
        })
        .or_else(|| self.lock().snapshot.clone())
    }
}
