//! Bridging quest item references to the item catalog and to what the player owns. Ported from the
//! item-related parts of `quest_service.py`: `load_items_index`/`get_concrete_item_ids` (item
//! "family" grouping), `build_item_payload`, and `merge_item_family_holdings`.
//!
//! Python derived an item's family (e.g. every rarity grade of "Bandage") from a `darkerdb_archetype`
//! field in `items.json`, falling back to stripping a trailing grade suffix only when that metadata
//! was absent. `game_data::ItemCatalog` never carries that metadata at all (its schema has no
//! archetype field — see `game-data/src/items.rs`), so this crate always uses that fallback. Quest
//! objectives reference an archetype (e.g. "Bandage"), while owned items carry a concrete grade id
//! (e.g. "Bandage_4001"); [`ItemFamilyIndex`] resolves one to the other.

use std::collections::HashMap;

use game_data::{Item, ItemCatalog};
use state::Character;

/// One archetype's concrete grades, e.g. "Bandage" -> ["Bandage_1001", ..., "Bandage_4001"].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFamily {
    pub archetype_id: String,
    /// The lowest-rarity concrete item, used to display the family before a specific grade is
    /// known (mirrors Python's tie-break: lowest rarity, then item id).
    pub representative_item_id: String,
    /// Every concrete item id in this family, sorted.
    pub concrete_item_ids: Vec<String>,
}

/// Groups a [`game_data::ItemCatalog`] by archetype so quest objectives (which name an archetype)
/// can be resolved to the concrete grade ids a character's stash actually contains.
#[derive(Debug, Default)]
pub struct ItemFamilyIndex {
    families: HashMap<String, ItemFamily>,
}

impl ItemFamilyIndex {
    pub fn build(catalog: &ItemCatalog) -> Self {
        let mut groups: HashMap<String, Vec<&Item>> = HashMap::new();
        for item in catalog.iter() {
            groups.entry(derive_archetype(&item.id)).or_default().push(item);
        }

        let mut families = HashMap::with_capacity(groups.len());
        for (archetype_id, candidates) in groups {
            let mut concrete_item_ids: Vec<String> = candidates.iter().map(|item| item.id.clone()).collect();
            concrete_item_ids.sort();
            concrete_item_ids.dedup();
            let representative = candidates
                .iter()
                .min_by(|a, b| a.rarity.cmp(&b.rarity).then_with(|| a.id.cmp(&b.id)))
                .expect("a group always has at least the item that created it");
            families.insert(
                archetype_id.clone(),
                ItemFamily { archetype_id, representative_item_id: representative.id.clone(), concrete_item_ids },
            );
        }
        Self { families }
    }

    /// The family for `item_id` when it names a whole archetype (e.g. "Bandage").
    pub fn family(&self, item_id: &str) -> Option<&ItemFamily> {
        self.families.get(item_id)
    }

    /// Resolve a quest item reference to every concrete stash item id it represents. Mirrors
    /// `QuestService.get_concrete_item_ids`: an archetype expands to its whole family, and anything
    /// else (a concrete id, or an id this catalog doesn't know) resolves to just itself.
    pub fn concrete_item_ids(&self, item_id: &str) -> Vec<String> {
        let normalized = item_id.trim();
        if normalized.is_empty() {
            return Vec::new();
        }
        match self.families.get(normalized) {
            Some(family) if !family.concrete_item_ids.is_empty() => family.concrete_item_ids.clone(),
            _ => vec![normalized.to_string()],
        }
    }
}

/// Strip a trailing grade suffix (`_1001` through `_8001`, one per rarity tier) from a concrete
/// item id. An id without that suffix is its own singleton archetype.
fn derive_archetype(item_id: &str) -> String {
    let len = item_id.len();
    if len >= 5 {
        let suffix = &item_id[len - 5..];
        let mut chars = suffix.chars();
        let (underscore, grade, rest) = (chars.next(), chars.next(), chars.as_str());
        if underscore == Some('_') && rest == "001" && matches!(grade, Some(c) if ('1'..='8').contains(&c)) {
            return item_id[..len - 5].to_string();
        }
    }
    item_id.to_string()
}

/// How much a character owns of a quest's item reference, by summing every concrete grade
/// [`ItemFamilyIndex::concrete_item_ids`] resolves it to. This is the crate's bridge from "what a
/// quest wants" to "what `state::Character` reports the player has". Items in the locked seasonal
/// stash are only a preview, not the player's, so they never count.
pub fn owned_count(character: &Character, families: &ItemFamilyIndex, item_id: &str) -> u32 {
    let concrete_ids = families.concrete_item_ids(item_id);
    character
        .items
        .iter()
        .filter(|owned| !state::is_off_limits(owned.inventory_id))
        .filter(|owned| concrete_ids.iter().any(|id| id == &owned.item_id))
        .map(|owned| owned.count)
        .sum()
}

/// Item metadata as the UI needs it to build a display payload. Mirrors the shape of the dicts
/// `QuestService.build_item_payload` reads from (either an `items_index` entry, or an ad hoc dict).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemInfo {
    pub item_id: Option<String>,
    pub name: Option<String>,
    pub rarity: Option<String>,
    pub item_type: Option<String>,
    /// An icon URL already resolved by the caller (or an upstream API).
    pub icon: Option<String>,
    /// Same as `icon`; Python checks `icon_url` first and falls back to `icon`.
    pub icon_url: Option<String>,
    /// A path inside the local icon pack, resolved to a URL via `icon_url_builder` when present.
    pub icon_path: Option<String>,
    pub archetype: Option<String>,
    pub representative_item_id: Option<String>,
}

/// A serializable item payload for the UI, as returned by `QuestService.build_item_payload`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemPayload {
    pub item_id: Option<String>,
    pub name: Option<String>,
    pub rarity: Option<String>,
    pub item_type: Option<String>,
    pub icon: Option<String>,
    pub icon_path: Option<String>,
    pub archetype: Option<String>,
    pub representative_item_id: Option<String>,
}

/// Turns a local icon path into a servable URL; returns `None` if that fails (Python swallows the
/// exception the build callback raised the same way).
pub type IconUrlBuilder<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Build a UI-facing item payload. `icon_url_builder` turns a local icon path into a servable URL;
/// return `None` from it if that fails (Python swallows the exception the same way). With no
/// `item_info` at all, returns the same "Unknown Item" placeholder Python does.
pub fn build_item_payload(item_info: Option<&ItemInfo>, icon_url_builder: Option<&IconUrlBuilder<'_>>) -> ItemPayload {
    let Some(info) = item_info else {
        return ItemPayload {
            item_id: None,
            name: Some("Unknown Item".to_string()),
            rarity: Some("Unknown".to_string()),
            item_type: None,
            icon: None,
            icon_path: None,
            archetype: None,
            representative_item_id: None,
        };
    };

    let icon_path = info.icon_path.clone();
    let mut icon = info.icon_url.clone().or_else(|| info.icon.clone());
    if let (Some(path), Some(builder)) = (icon_path.as_deref(), icon_url_builder) {
        icon = builder(path);
    }

    ItemPayload {
        item_id: info.item_id.clone(),
        name: info.name.clone(),
        rarity: info.rarity.clone(),
        item_type: info.item_type.clone(),
        icon,
        icon_path,
        archetype: info.archetype.clone(),
        representative_item_id: info.representative_item_id.clone(),
    }
}

/// One character's stash slots holding a concrete item grade, as reported by wherever the app
/// gathers cross-character holdings (e.g. a listing/market feature, not this crate).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StashHolding {
    pub stash_id: Option<String>,
    pub count: u32,
    /// The concrete item id this holding is for; [`merge_item_family_holdings`] fills it in, so
    /// callers building the per-item input don't need to set it themselves.
    pub item_id: Option<String>,
}

/// One character's total holdings of a concrete item grade, before family merging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CharacterHolding {
    pub character_id: Option<String>,
    pub character_name: Option<String>,
    pub character_class: Option<String>,
    pub character_level: Option<u32>,
    pub last_update: Option<u64>,
    pub total: u32,
    pub stashes: Vec<StashHolding>,
}

/// Merge per-concrete-grade holdings into one entry per character across a whole item family, so
/// "how much Bandage does each character have" sums every grade instead of listing them separately.
/// Mirrors `QuestService.merge_item_family_holdings`.
pub fn merge_item_family_holdings(
    holdings_by_item: &HashMap<String, Vec<CharacterHolding>>,
    concrete_item_ids: &[String],
) -> Vec<CharacterHolding> {
    let mut by_character: HashMap<String, CharacterHolding> = HashMap::new();

    for concrete_id in concrete_item_ids {
        let Some(entries) = holdings_by_item.get(concrete_id) else { continue };
        for raw in entries {
            let character_key = raw
                .character_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .or(raw.character_name.as_deref())
                .filter(|name| !name.is_empty())
                .unwrap_or("Unknown")
                .to_string();

            let entry = by_character.entry(character_key).or_insert_with(|| CharacterHolding {
                character_id: raw.character_id.clone(),
                character_name: raw.character_name.clone(),
                character_class: raw.character_class.clone(),
                character_level: raw.character_level,
                last_update: raw.last_update,
                total: 0,
                stashes: Vec::new(),
            });
            entry.total += raw.total;
            for stash in &raw.stashes {
                entry.stashes.push(StashHolding {
                    stash_id: stash.stash_id.clone(),
                    count: stash.count,
                    item_id: Some(concrete_id.clone()),
                });
            }
        }
    }

    let mut merged: Vec<CharacterHolding> = by_character.into_values().collect();
    for entry in &mut merged {
        entry.stashes.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.stash_id.clone().unwrap_or_default().cmp(&b.stash_id.clone().unwrap_or_default()))
                .then_with(|| a.item_id.clone().unwrap_or_default().cmp(&b.item_id.clone().unwrap_or_default()))
        });
    }
    merged.sort_by(|a, b| {
        b.total.cmp(&a.total).then_with(|| {
            let (left, right) =
                (a.character_name.clone().unwrap_or_default(), b.character_name.clone().unwrap_or_default());
            left.to_lowercase().cmp(&right.to_lowercase())
        })
    });
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn catalog_with(entries: &[(&str, &str)]) -> ItemCatalog {
        let mut map = serde_json::Map::new();
        for (id, rarity) in entries {
            map.insert(
                (*id).to_string(),
                serde_json::json!({"name": "Bandage", "rarity": rarity, "type": "Utility"}),
            );
        }
        ItemCatalog::from_json(&serde_json::Value::Object(map).to_string()).expect("valid catalog json")
    }

    #[test]
    fn item_index_maps_archetype_to_all_concrete_grades() {
        let catalog = catalog_with(&[("Bandage_4001", "Rare"), ("Bandage_1001", "Poor")]);
        let families = ItemFamilyIndex::build(&catalog);

        let family = families.family("Bandage").expect("Bandage family exists");
        assert_eq!(family.representative_item_id, "Bandage_1001");
        assert_eq!(family.concrete_item_ids, vec!["Bandage_1001".to_string(), "Bandage_4001".to_string()]);
        assert_eq!(
            families.concrete_item_ids("Bandage"),
            vec!["Bandage_1001".to_string(), "Bandage_4001".to_string()]
        );
    }

    #[test]
    fn concrete_item_ids_falls_back_to_itself_when_no_family_is_known() {
        let families = ItemFamilyIndex::build(&ItemCatalog::default());
        assert_eq!(families.concrete_item_ids("Unknown_9001"), vec!["Unknown_9001".to_string()]);
        assert!(families.concrete_item_ids("").is_empty());
    }

    #[test]
    fn owned_count_sums_every_grade_in_the_family() {
        let catalog = catalog_with(&[("Bandage_4001", "Rare"), ("Bandage_1001", "Poor")]);
        let families = ItemFamilyIndex::build(&catalog);
        let character = Character {
            id: "one".to_string(),
            name: "Cleric".to_string(),
            class: "Cleric".to_string(),
            level: 1,
            items: vec![
                make_owned_item("Bandage_1001", 2),
                make_owned_item("Bandage_4001", 3),
                make_owned_item("HeaterShield_5001", 1),
                state::OwnedItem { inventory_id: state::LOCKED_SEASONAL_STASH, ..make_owned_item("Bandage_1001", 9) },
            ],
            storages: Vec::new(),
        };

        assert_eq!(owned_count(&character, &families, "Bandage"), 5, "the locked seasonal stash never counts");
        assert_eq!(owned_count(&character, &families, "HeaterShield_5001"), 1);
    }

    fn make_owned_item(item_id: &str, count: u32) -> state::OwnedItem {
        state::OwnedItem {
            unique_id: 0,
            item_id: item_id.to_string(),
            count,
            contents: 0,
            inventory_id: state::stash::BAG,
            slot_id: None,
            base: Vec::new(),
            rolls: Vec::new(),
            loot_state: 0,
            tradable: true,
        }
    }

    #[test]
    fn build_item_payload_uses_remote_icon_when_local_item_is_missing() {
        let info = ItemInfo {
            item_id: Some("LuckPotion_4001".to_string()),
            name: Some("Luck Potion".to_string()),
            rarity: Some("Rare".to_string()),
            item_type: Some("Utility".to_string()),
            icon_url: Some("https://cdn.example.test/luck-potion.webp".to_string()),
            ..Default::default()
        };

        let payload = build_item_payload(Some(&info), None);

        assert_eq!(payload.icon.as_deref(), Some("https://cdn.example.test/luck-potion.webp"));
    }

    #[test]
    fn build_item_payload_with_no_info_is_the_unknown_item_placeholder() {
        let payload = build_item_payload(None, None);
        assert_eq!(payload.name.as_deref(), Some("Unknown Item"));
        assert_eq!(payload.rarity.as_deref(), Some("Unknown"));
    }

    #[test]
    fn item_family_holdings_merge_grades_per_character() {
        let mut holdings: HashMap<String, Vec<CharacterHolding>> = HashMap::new();
        holdings.insert(
            "Bandage_1001".to_string(),
            vec![CharacterHolding {
                character_id: Some("one".to_string()),
                character_name: Some("Cleric".to_string()),
                total: 2,
                stashes: vec![StashHolding { stash_id: Some("0".to_string()), count: 2, item_id: None }],
                ..Default::default()
            }],
        );
        holdings.insert(
            "Bandage_4001".to_string(),
            vec![CharacterHolding {
                character_id: Some("one".to_string()),
                character_name: Some("Cleric".to_string()),
                total: 3,
                stashes: vec![StashHolding { stash_id: Some("1".to_string()), count: 3, item_id: None }],
                ..Default::default()
            }],
        );

        let merged = merge_item_family_holdings(
            &holdings,
            &["Bandage_1001".to_string(), "Bandage_4001".to_string()],
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].total, 5);
        let item_ids: HashSet<_> = merged[0].stashes.iter().filter_map(|s| s.item_id.clone()).collect();
        assert_eq!(item_ids, HashSet::from(["Bandage_1001".to_string(), "Bandage_4001".to_string()]));
    }
}
