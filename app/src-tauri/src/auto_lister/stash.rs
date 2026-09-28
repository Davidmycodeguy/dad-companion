//! A character's stash in the shape the lister reads: one JSON object per item, keyed like the
//! enhanced items DnDTools built (`itemId`, `itemUniqueId`, `slotId`, `rarity`, `pp`/`sp` rolls, …).

use std::collections::HashMap;

use serde_json::{json, Value};

use game_data::ItemCatalog;
use state::{is_off_limits, Character, OwnedItem};

/// Stash tabs items can be listed from: the bag and the storage tabs the Marketplace shows as
/// icons. Equipment is never sold from, and the locked seasonal stash is never touched.
pub fn listable_stash_ids(character: &Character) -> Vec<u32> {
    character
        .stash_ids()
        .into_iter()
        .filter(|&id| id == state::stash::BAG || (state::stash::STORAGE..GEAR_SET_FIRST_ID).contains(&id))
        .filter(|&id| !is_off_limits(id))
        .collect()
}

/// Gear-set presets start at this inventory id; they are not stash tabs.
const GEAR_SET_FIRST_ID: u32 = 100;

/// The requested stash tabs as lister items, never including the locked seasonal stash.
pub fn stashes_json(
    character: &Character,
    catalog: &ItemCatalog,
    stash_ids: &[String],
) -> HashMap<String, Vec<Value>> {
    stash_ids
        .iter()
        .filter_map(|id| {
            id.parse::<u32>()
                .ok()
                .filter(|&n| !is_off_limits(n))
                .map(|n| (id, n))
        })
        .map(|(id, inventory_id)| {
            let items = character
                .stash(inventory_id)
                .iter()
                .map(|item| item_json(item, catalog))
                .collect();
            (id.clone(), items)
        })
        .collect()
}

/// The stash tab ids in the order the Marketplace shows their icons.
pub fn tab_mapping(character: &Character) -> Vec<i64> {
    let ids: Vec<String> = character.stash_ids().iter().map(u32::to_string).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    input_tab_order(&refs)
}

/// Marketplace tab icons follow the account's stash ids in ascending order (from the bag's icon on).
fn input_tab_order(stash_ids: &[&str]) -> Vec<i64> {
    let mut ids: Vec<i64> = stash_ids
        .iter()
        .filter_map(|raw| raw.parse::<i64>().ok())
        .filter(|id| (i64::from(state::stash::STORAGE)..i64::from(GEAR_SET_FIRST_ID)).contains(id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn item_json(item: &OwnedItem, catalog: &ItemCatalog) -> Value {
    let meta = catalog.get(&item.item_id);
    let pairs = |stats: &[(String, i64)]| {
        stats
            .iter()
            .map(|(stat, value)| json!([stat, value]))
            .collect::<Vec<_>>()
    };
    json!({
        "name": meta.map_or_else(|| item.item_id.clone(), |m| m.name.clone()),
        "itemId": item.item_id,
        "itemUniqueId": item.unique_id.to_string(),
        "slotId": item.slot_id.unwrap_or(0),
        "itemCount": item.count.max(1),
        "rarity": meta.map_or("Unknown", |m| m.rarity.name()),
        "width": meta.map_or(1, |m| m.width.max(1)),
        "height": meta.map_or(1, |m| m.height.max(1)),
        "pp": pairs(&item.base),
        "sp": pairs(&item.rolls),
        "vendor_price": meta.map_or(0, |m| m.vendor_price),
        "max_stack_size": meta.map_or(1, |m| m.max_stack.max(1)),
        "slot_type": meta.map_or("", |m| m.slot_type.as_str()),
        "originalData": { "tradable": u8::from(item.tradable), "lootState": item.loot_state },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::LOCKED_SEASONAL_STASH;

    fn owned(unique_id: u64, inventory_id: u32) -> OwnedItem {
        OwnedItem {
            unique_id,
            item_id: "HeaterShield_4001".into(),
            count: 1,
            contents: 0,
            inventory_id,
            slot_id: Some(3),
            base: vec![("ArmorRating".into(), 60)],
            rolls: vec![("Strength".into(), 2)],
            loot_state: 0,
            tradable: true,
        }
    }

    fn character(items: Vec<OwnedItem>) -> Character {
        Character {
            id: "1".into(),
            name: "Tester".into(),
            class: "Fighter".into(),
            level: 20,
            items,
            storages: vec![4, 5, 6, 7, 8, 9],
        }
    }

    #[test]
    fn the_locked_seasonal_stash_is_never_listable() {
        let c = character(vec![
            owned(1, 2),
            owned(2, 4),
            owned(3, LOCKED_SEASONAL_STASH),
            owned(4, 3),
        ]);
        let ids = listable_stash_ids(&c);
        assert!(ids.contains(&2) && ids.contains(&4));
        assert!(
            !ids.contains(&LOCKED_SEASONAL_STASH),
            "locked stash offered: {ids:?}"
        );
        assert!(!ids.contains(&3), "equipment offered: {ids:?}");
    }

    #[test]
    fn asking_for_the_locked_stash_returns_nothing_from_it() {
        let c = character(vec![owned(3, LOCKED_SEASONAL_STASH)]);
        let catalog = ItemCatalog::from_json("{}").expect("an empty catalog");
        let stashes = stashes_json(&c, &catalog, &[LOCKED_SEASONAL_STASH.to_string()]);
        assert!(stashes.is_empty());
    }

    #[test]
    fn items_carry_the_keys_the_lister_reads() {
        let c = character(vec![owned(7, 4)]);
        let catalog = ItemCatalog::from_json("{}").expect("an empty catalog");
        let stashes = stashes_json(&c, &catalog, &["4".into()]);
        let item = &stashes["4"][0];
        assert_eq!(item["itemUniqueId"], "7");
        assert_eq!(item["slotId"], 3);
        assert_eq!(item["sp"], json!([["Strength", 2]]));
        assert_eq!(item["originalData"]["tradable"], 1);
    }

    #[test]
    fn tab_icons_follow_stash_ids_in_order() {
        let c = character(vec![owned(1, 6), owned(2, 4), owned(3, 2), owned(4, 20)]);
        assert_eq!(tab_mapping(&c), vec![4, 5, 6, 7, 8, 9, 20]);
    }
}
