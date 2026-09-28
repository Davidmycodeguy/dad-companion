//! What the player owns of what quests ask for: every character's items (each item once, and never
//! the locked seasonal stash, whose items are only a preview), matched by item family or type and
//! lowest rarity. Quests take looted items only (DnDTools counted `lootState` 2, "Looted"); items
//! that fit but came another way (bought, crafted, traded) are counted apart.

use std::collections::{HashMap, HashSet};

use game_data::{ItemCatalog, Rarity};
use state::Character;

use crate::items::ItemFamilyIndex;
use crate::loot::LOOT_STATE_LOOTED;
use crate::view::model::{Owned, Place};

/// What a quest asks for: an item family (or one concrete item), or any item of a type, at a
/// lowest rarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Requirement<'r> {
    pub item_id: Option<&'r str>,
    pub item_type: Option<&'r str>,
    pub min_rarity: Option<Rarity>,
}

struct Holding<'a> {
    character: &'a str,
    inventory_id: u32,
    count: u32,
    looted: bool,
    rarity: Rarity,
    item_type: &'a str,
}

/// Every counted item, indexed by catalog id.
pub(crate) struct OwnedIndex<'a> {
    holdings: Vec<Holding<'a>>,
    by_item: HashMap<&'a str, Vec<usize>>,
    families: &'a ItemFamilyIndex,
}

impl<'a> OwnedIndex<'a> {
    pub fn build(characters: &'a [Character], catalog: &'a ItemCatalog, families: &'a ItemFamilyIndex) -> Self {
        let mut seen = HashSet::new();
        let mut holdings = Vec::new();
        let mut by_item: HashMap<&'a str, Vec<usize>> = HashMap::new();
        for character in characters {
            for item in &character.items {
                if state::is_off_limits(item.inventory_id) {
                    continue;
                }
                // An item the game reports for several characters (a shared stash) counts once.
                if item.unique_id != 0 && !seen.insert(item.unique_id) {
                    continue;
                }
                let entry = catalog.get(&item.item_id);
                by_item.entry(item.item_id.as_str()).or_default().push(holdings.len());
                holdings.push(Holding {
                    character: &character.name,
                    inventory_id: item.inventory_id,
                    count: item.count.max(1),
                    looted: item.loot_state == LOOT_STATE_LOOTED,
                    rarity: entry.map_or(Rarity::Unknown, |e| e.rarity),
                    item_type: entry.map_or("", |e| e.item_type.as_str()),
                });
            }
        }
        Self { holdings, by_item, families }
    }

    /// How much of `requirement` the player owns, and where. `stash_label` names a stash tab.
    pub fn count(&self, requirement: &Requirement<'_>, stash_label: &dyn Fn(u32) -> String) -> Owned {
        let matching: Vec<&Holding<'a>> = match (requirement.item_id, requirement.item_type) {
            (Some(item_id), _) => self
                .families
                .concrete_item_ids(item_id)
                .iter()
                .flat_map(|id| self.by_item.get(id.as_str()).into_iter().flatten())
                .map(|&index| &self.holdings[index])
                .collect(),
            (None, Some(item_type)) => {
                self.holdings.iter().filter(|holding| holding.item_type.eq_ignore_ascii_case(item_type)).collect()
            }
            (None, None) => Vec::new(),
        };

        let mut owned = Owned::default();
        let mut places: HashMap<(&str, u32), (u32, u32)> = HashMap::new();
        for holding in matching.into_iter().filter(|holding| meets(holding.rarity, requirement.min_rarity)) {
            let place = places.entry((holding.character, holding.inventory_id)).or_default();
            if holding.looted {
                owned.usable += holding.count;
                place.0 += holding.count;
            } else {
                owned.other += holding.count;
                place.1 += holding.count;
            }
        }
        owned.places = places
            .into_iter()
            .map(|((character, inventory_id), (usable, other))| Place {
                character: character.to_string(),
                stash: stash_label(inventory_id),
                usable,
                other,
            })
            .collect();
        owned.places.sort_by(|a, b| {
            (b.usable, b.other).cmp(&(a.usable, a.other)).then_with(|| (&a.character, &a.stash).cmp(&(&b.character, &b.stash)))
        });
        owned
    }
}

/// An item of `rarity` counts for a requirement of at least `min`. Items the catalog doesn't know
/// (rarity Unknown) always count.
fn meets(rarity: Rarity, min: Option<Rarity>) -> bool {
    match min {
        Some(min) => rarity >= min,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::OwnedItem;

    fn catalog() -> ItemCatalog {
        let json = serde_json::json!({
            "Diamond_3001": {"name": "Diamond", "rarity": "Uncommon", "type": "Misc"},
            "Diamond_4001": {"name": "Diamond", "rarity": "Rare", "type": "Misc"},
            "Diamond_5001": {"name": "Diamond", "rarity": "Epic", "type": "Misc"},
            "HeaterShield_2001": {"name": "Heater Shield", "rarity": "Common", "type": "Armor"},
        });
        ItemCatalog::from_json(&json.to_string()).expect("catalog")
    }

    fn item(unique_id: u64, item_id: &str, count: u32, inventory_id: u32, loot_state: i32) -> OwnedItem {
        OwnedItem {
            unique_id,
            item_id: item_id.to_string(),
            count,
            contents: 0,
            inventory_id,
            slot_id: None,
            base: Vec::new(),
            rolls: Vec::new(),
            loot_state,
            tradable: true,
        }
    }

    fn character(name: &str, items: Vec<OwnedItem>) -> Character {
        Character { id: name.to_string(), name: name.to_string(), items, ..Character::default() }
    }

    fn label(inventory_id: u32) -> String {
        format!("Stash {inventory_id}")
    }

    #[test]
    fn counts_looted_items_of_the_rarity_or_higher_once_outside_the_locked_stash() {
        let catalog = catalog();
        let families = ItemFamilyIndex::build(&catalog);
        let characters = vec![
            character(
                "Wizard",
                vec![
                    item(1, "Diamond_4001", 2, 4, LOOT_STATE_LOOTED),
                    item(2, "Diamond_5001", 1, 2, LOOT_STATE_LOOTED),
                    item(3, "Diamond_3001", 5, 4, LOOT_STATE_LOOTED),
                    item(4, "Diamond_4001", 3, 5, 3),
                    item(5, "Diamond_4001", 9, state::LOCKED_SEASONAL_STASH, LOOT_STATE_LOOTED),
                ],
            ),
            // The same shared-stash item reported again for another character.
            character("Fighter", vec![item(1, "Diamond_4001", 2, 4, LOOT_STATE_LOOTED)]),
        ];
        let index = OwnedIndex::build(&characters, &catalog, &families);

        let rare = Requirement { item_id: Some("Diamond"), item_type: None, min_rarity: Some(Rarity::Rare) };
        let owned = index.count(&rare, &label);
        assert_eq!((owned.usable, owned.other), (3, 3));
        let first = &owned.places[0];
        assert_eq!((first.character.as_str(), first.stash.as_str(), first.usable), ("Wizard", "Stash 4", 2));

        let any = Requirement { min_rarity: None, ..rare };
        assert_eq!(index.count(&any, &label).usable, 8);
    }

    #[test]
    fn item_type_requirements_count_every_item_of_that_type() {
        let catalog = catalog();
        let families = ItemFamilyIndex::build(&catalog);
        let characters = vec![character("Wizard", vec![item(7, "HeaterShield_2001", 1, 3, LOOT_STATE_LOOTED)])];
        let index = OwnedIndex::build(&characters, &catalog, &families);

        let armor = Requirement { item_id: None, item_type: Some("armor"), min_rarity: Some(Rarity::Common) };
        assert_eq!(index.count(&armor, &label).usable, 1);
        let uncommon = Requirement { min_rarity: Some(Rarity::Uncommon), ..armor };
        assert_eq!(index.count(&uncommon, &label), Owned::default());
        let nothing = Requirement { item_id: None, item_type: None, min_rarity: None };
        assert_eq!(index.count(&nothing, &label), Owned::default());
    }
}
