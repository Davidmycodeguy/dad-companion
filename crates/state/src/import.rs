//! Characters saved by DnDTools: the game's character info message as JSON (protobuf's
//! MessageToDict of S2C_LOBBY_CHARACTER_INFO_RES), one file per character.

use serde_json::Value;

use crate::stash::{Character, OwnedItem};

const ITEM_ID_PREFIX: &str = "Id_Item_";
const PROPERTY_PREFIX: &str = "Effect_";
const CLASS_PREFIX: &str = "Id_PlayerCharacter_";

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("not a saved character (no characterDataBase)")]
    NotACharacter,
}

/// A character from a DnDTools character file's JSON.
pub fn character_from_saved(json: &Value) -> Result<Character, ImportError> {
    let base = json.get("characterDataBase").filter(|b| b.is_object()).ok_or(ImportError::NotACharacter)?;
    let mut items: Vec<OwnedItem> = array(base, "CharacterItemList").iter().filter_map(|item| owned_item(item, None)).collect();
    let mut storages = Vec::new();
    for storage in array(base, "CharacterStorageInfos") {
        let inventory_id = number(storage, "inventoryId").map(|n| n as u32);
        storages.extend(inventory_id);
        items.extend(array(storage, "CharacterStorageItemList").iter().filter_map(|item| owned_item(item, inventory_id)));
    }
    Ok(Character {
        id: text(base, "characterId"),
        name: base.get("nickName").map(|n| text(n, "originalNickName")).unwrap_or_default(),
        class: after(&text(base, "characterClass"), CLASS_PREFIX),
        level: number(base, "level").unwrap_or(0) as u32,
        items,
        storages,
    })
}

/// An item; `storage_id` is the stash it was listed under (the item's own inventoryId wins).
fn owned_item(item: &Value, storage_id: Option<u32>) -> Option<OwnedItem> {
    let item_id = after(&text(item, "itemId"), ITEM_ID_PREFIX);
    if item_id.is_empty() {
        return None;
    }
    Some(OwnedItem {
        // 64-bit ids are strings in MessageToDict's JSON.
        unique_id: text(item, "itemUniqueId").parse().ok().or_else(|| number(item, "itemUniqueId").map(|n| n as u64))?,
        item_id,
        count: number(item, "itemCount").map_or(1, |n| n.max(1) as u32),
        contents: number(item, "itemContentsCount").map_or(0, |n| n.max(0) as u32),
        inventory_id: number(item, "inventoryId").map(|n| n as u32).or(storage_id)?,
        slot_id: number(item, "slotId").map(|n| n as u32),
        base: stats(item, "primaryPropertyArray"),
        rolls: stats(item, "secondaryPropertyArray"),
        loot_state: number(item, "lootState").unwrap_or(0) as i32,
        tradable: number(item, "tradable") == Some(1),
    })
}

fn stats(item: &Value, key: &str) -> Vec<(String, i64)> {
    array(item, key)
        .iter()
        .map(|p| (after(&text(p, "propertyTypeId"), PROPERTY_PREFIX), number(p, "propertyValue").unwrap_or(0)))
        .filter(|(id, _)| !id.is_empty())
        .collect()
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn number(value: &Value, key: &str) -> Option<i64> {
    match value.get(key)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// The part of `id` after `prefix` ("DesignDataItem:Id_Item_Sword_5001" -> "Sword_5001").
fn after(id: &str, prefix: &str) -> String {
    id.rsplit(prefix).next().unwrap_or(id).to_owned()
}
