//! Items in stashes: where each one sits and which stashes may be touched at all.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The player's bag (inventory), 10x5 cells.
pub const BAG: u32 = 2;
/// Equipped items.
pub const EQUIPMENT: u32 = 3;
/// The first stash tab; tabs 4..=9 are the stash and the purchased stashes.
pub const STORAGE: u32 = 4;
pub const LAST_PURCHASED_STORAGE: u32 = 9;
/// The seasonal shared stash.
pub const SHARED_STASH_SEASONAL: u32 = 20;
/// The stash DnDTools called "Shared Stash". On this account it is the locked Seasonal Shared
/// Stash: its potions, lockpicks and gold pouches are only a preview of what unlocking (a purchase)
/// gives. Nothing may list, sell, move or count them, and its Unlock button must never be clicked.
pub const LOCKED_SEASONAL_STASH: u32 = 30;

/// Stash tabs are 12 cells wide and 20 tall.
const STASH_SIZE: (u32, u32) = (12, 20);
const BAG_SIZE: (u32, u32) = (10, 5);

/// (width, height) in cells of a stash that has a grid, or None (equipment and others).
pub fn grid_size(inventory_id: u32) -> Option<(u32, u32)> {
    match inventory_id {
        BAG => Some(BAG_SIZE),
        STORAGE..=LOCKED_SEASONAL_STASH => Some(STASH_SIZE),
        _ => None,
    }
}

/// The (x, y) cell of slot `slot_id` in a grid `width` cells wide (slots run row by row).
pub fn slot_cell(slot_id: u32, width: u32) -> (u32, u32) {
    (slot_id % width.max(1), slot_id / width.max(1))
}

/// Stashes the app must never touch (list from, sell from, sort, or count as the player's).
pub fn is_off_limits(inventory_id: u32) -> bool {
    inventory_id == LOCKED_SEASONAL_STASH
}

/// One item the player owns, as the game describes it (the SItem message).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedItem {
    pub unique_id: u64,
    /// The catalog's id, e.g. "HeaterShield_5001".
    pub item_id: String,
    pub count: u32,
    /// What a container holds (gold in a coin purse, pouch or bag); 0 for other items.
    #[serde(default)]
    pub contents: u32,
    /// Which stash holds it (see the constants above).
    pub inventory_id: u32,
    /// Its slot in that stash; None when the game left it out.
    pub slot_id: Option<u32>,
    /// Base stats, as (stat id, value) in game-data units.
    pub base: Vec<(String, i64)>,
    /// Random rolls, as (stat id, value) in game-data units.
    pub rolls: Vec<(String, i64)>,
    pub loot_state: i32,
    pub tradable: bool,
}

impl OwnedItem {
    /// Gold this item is (coins at face value, a coin purse, pouch, bag or chest by what it
    /// holds); None for anything that isn't gold.
    pub fn gold(&self) -> Option<u64> {
        if self.item_id == "GoldCoins" {
            Some(u64::from(self.count))
        } else if self.item_id.starts_with("GoldCoin") {
            Some(u64::from(self.contents) * u64::from(self.count.max(1)))
        } else {
            None
        }
    }
}

/// One character and everything it owns, from the game's character info.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub name: String,
    pub class: String,
    pub level: u32,
    pub items: Vec<OwnedItem>,
    /// Every stash tab the game listed for this character, empty ones included: the Marketplace
    /// shows an icon for each, so the lister needs them all to click the right one.
    #[serde(default)]
    pub storages: Vec<u32>,
}

impl Character {
    /// The items in stash `inventory_id`, each with a slot: items the game sent without one get
    /// the lowest free slot, as DnDTools did.
    pub fn stash(&self, inventory_id: u32) -> Vec<OwnedItem> {
        let mut items: Vec<OwnedItem> = self.items.iter().filter(|i| i.inventory_id == inventory_id).cloned().collect();
        let mut used: BTreeSet<u32> = items.iter().filter_map(|i| i.slot_id).collect();
        for item in items.iter_mut().filter(|i| i.slot_id.is_none()) {
            let slot = (0..).find(|s| !used.contains(s)).unwrap_or(0);
            used.insert(slot);
            item.slot_id = Some(slot);
        }
        items
    }

    /// Ids of the stashes that hold anything, and of every stash tab the game listed.
    pub fn stash_ids(&self) -> Vec<u32> {
        let ids: BTreeSet<u32> = self.items.iter().map(|i| i.inventory_id).chain(self.storages.iter().copied()).collect();
        ids.into_iter().collect()
    }
}
