//! The planning-time item model: everything the planner needs to sort and place one item. Port of
//! `item.py`'s `Item` (renamed `SortItem` here since `game_data::Item` is already taken by the
//! catalog entry).

use game_data::{ItemCatalog, Rarity};
use state::OwnedItem;

/// An item as the planner sees it. Python's `Item` is built by `Storage.load` from the raw packet
/// plus `item_data_manager` lookups; this is built by [`SortItem::from_owned`] from `OwnedItem`
/// plus `ItemCatalog`. It intentionally excludes fields Python carries but the planner never reads
/// (e.g. `vendor_price`, used only for UI display) and the `stacked` flag (Python sets it after a
/// stacking merge, but by then the item has already been dropped from every collection the planner
/// still iterates, so the flag is never actually consulted — see `StackingEngine`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortItem {
    pub unique_id: u64,
    /// The catalog id, e.g. "HeaterShield_5001" — the stacking engine groups by this and rarity.
    pub item_id: String,
    pub name: String,
    pub rarity: Rarity,
    /// Equipment slot ("Head", "Chest", ...); empty for loot. Python calls the sortable field
    /// `"slot"`, but it compares this, not the grid position.
    pub slot_type: String,
    pub width: u32,
    pub height: u32,
    pub quantity: u32,
    pub max_stack: u32,
}

impl SortItem {
    /// Builds a planning item from the game's raw item plus catalog data. Never fails: an item
    /// missing from the catalog (assets not yet updated) still gets a placeholder — 1x1, Common,
    /// named after its raw id — so the sort can include it instead of dropping it silently. Port
    /// of `Storage.load`'s fallback (there, an unrecognized *rarity string* on a known item
    /// defaults to Common; here the whole catalog entry can be missing, so the same fallback
    /// covers both cases).
    pub fn from_owned(owned: &OwnedItem, catalog: &ItemCatalog) -> Self {
        let quantity = owned.count.max(1);
        match catalog.get(&owned.item_id) {
            Some(catalog_item) => Self {
                unique_id: owned.unique_id,
                item_id: owned.item_id.clone(),
                name: if catalog_item.name.is_empty() {
                    owned.item_id.clone()
                } else {
                    catalog_item.name.clone()
                },
                rarity: catalog_item.rarity,
                slot_type: catalog_item.slot_type.clone(),
                width: catalog_item.width.max(1),
                height: catalog_item.height.max(1),
                quantity,
                max_stack: catalog_item.max_stack.max(1),
            },
            None => Self {
                unique_id: owned.unique_id,
                item_id: owned.item_id.clone(),
                name: owned.item_id.clone(),
                rarity: Rarity::Common,
                slot_type: String::new(),
                width: 1,
                height: 1,
                quantity,
                max_stack: 1,
            },
        }
    }

    pub fn area(&self) -> u32 {
        self.width * self.height
    }

    pub fn longest_side(&self) -> u32 {
        self.width.max(self.height)
    }

    /// Items that can share one stack: same catalog id and rarity. Python:
    /// `(getattr(item, "item_id", None), item.rarity)`.
    pub fn stack_key(&self) -> (&str, Rarity) {
        (&self.item_id, self.rarity)
    }
}

/// Rarity's position in item quality, lowest first. Port of `Item._RARITY_RANK`. Deliberately not
/// `Rarity`'s own declared order: that enum's `Unknown` sorts *last* (a catch-all, not a quality
/// tier), but Python ranks an unrecognized rarity the same as its lowest tier, `"none"` (rank 0).
pub fn rarity_rank(rarity: Rarity) -> u8 {
    match rarity {
        Rarity::Unknown => 0,
        Rarity::Poor => 1,
        Rarity::Common => 2,
        Rarity::Uncommon => 3,
        Rarity::Rare => 4,
        Rarity::Epic => 5,
        Rarity::Legendary => 6,
        Rarity::Unique => 7,
        Rarity::Artifact => 8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rarity_rank_treats_unknown_as_the_lowest_tier() {
        assert_eq!(rarity_rank(Rarity::Unknown), 0);
        assert!(rarity_rank(Rarity::Unknown) < rarity_rank(Rarity::Poor));
        assert!(rarity_rank(Rarity::Artifact) > rarity_rank(Rarity::Unique));
    }

    #[test]
    fn from_owned_falls_back_when_catalog_is_missing_the_item() {
        let catalog = ItemCatalog::from_json("{}").expect("empty catalog parses");
        let owned = OwnedItem {
            unique_id: 1,
            item_id: "Unreleased_9999".to_string(),
            count: 0,
            contents: 0,
            inventory_id: 4,
            slot_id: Some(0),
            base: Vec::new(),
            rolls: Vec::new(),
            loot_state: 0,
            tradable: true,
        };
        let item = SortItem::from_owned(&owned, &catalog);
        assert_eq!(item.name, "Unreleased_9999");
        assert_eq!(item.rarity, Rarity::Common);
        assert_eq!((item.width, item.height), (1, 1));
        assert_eq!(item.quantity, 1); // count of 0 is clamped up like Python's quantity
    }
}
