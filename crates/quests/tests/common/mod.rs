//! A small quest and item catalog shaped like the real ones, and helpers to build what the game
//! would have shown.

#![allow(dead_code)]

use game_data::ItemCatalog;
use quests::packet_handler::{CapturedMission, CapturedQuest};
use quests::view::ViewInputs;
use quests::{ItemFamilyIndex, QuestCatalog, QuestFlag, QuestProgress, TrackedState};
use serde_json::json;
use state::{Character, OwnedItem};

pub const LOOTED: i32 = 2;
pub const BOUGHT: i32 = 3;

pub fn quest_catalog() -> QuestCatalog {
    let quests = json!({"quests": [
        {"id": "TavernMaster_01", "title": "Is it you?", "merchant": "Tavern Master", "order": 3,
         "objectives": [{"type": "Kill", "count": 1, "monster": "Skeleton Footman"}], "rewards": []},
        {"id": "Alchemist_01", "title": "Marks of Malice", "merchant": "Alchemist", "prerequisite": "TavernMaster_01",
         "order": 132, "objectives": [{"type": "Fetch", "count": 2, "item_id": "Bandage"}],
         "rewards": [{"type": "Item", "count": 30, "item_id": "GoldCoins"}, {"type": "Experience", "count": 25},
                     {"type": "Item", "count": 1, "item_id": "SurgicalKit_2001"},
                     {"type": "Random", "count": 1, "item_type": "Armor", "rarity": "Uncommon"}]},
        {"id": "Alchemist_02", "title": "In the Name of Progress", "merchant": "Alchemist", "prerequisite": "Alchemist_01",
         "order": 133, "dungeons": ["Crypts"],
         "objectives": [{"type": "Kill", "count": 2, "monster": "Living Armor"},
                        {"type": "Fetch", "count": 2, "item_id": "Diamond", "rarity": "Rare"}], "rewards": []},
        {"id": "Alchemist_03", "title": "Whispers of the Past", "merchant": "Alchemist", "prerequisite": "Alchemist_02",
         "order": 134, "dungeons": ["Crypts"],
         "objectives": [{"type": "Explore", "count": 1, "module": "Ruins Keep"},
                        {"type": "Use Item", "count": 3, "item_id": "Bandage"},
                        {"type": "Survive", "count": 1}], "rewards": []},
        {"id": "Woodsman_05", "title": "The Use of a Forgotten Castle", "merchant": "Woodsman", "order": 96,
         "objectives": [{"type": "Fetch", "count": 1, "item_id": "Bavin"}, {"type": "Fetch", "count": 1, "item_id": "CrackedLog"},
                        {"type": "Fetch", "count": 1, "item_id": "CampfireKit"}], "rewards": []},
        {"id": "Cockatrice_01", "title": "Preparation for a Companion Cockatrice", "merchant": "Cockatrice", "order": 60,
         "objectives": [{"type": "Fetch", "count": 2, "item_id": "Bavin"}, {"type": "Fetch", "count": 2, "item_id": "CrackedLog"}],
         "rewards": []},
        {"id": "Huntress_Daily_01", "title": "Daily - One", "merchant": "Huntress Daily", "order": 400,
         "objectives": [{"type": "Kill", "count": 1, "monster": "Goblin Mage"}], "rewards": []},
        {"id": "Huntress_Daily_02", "title": "Daily - Two", "merchant": "Huntress Daily", "order": 401,
         "objectives": [{"type": "Fetch", "count": 1, "item_id": "BatWing"}], "rewards": []},
        {"id": "Huntress_Weekly_01", "title": "Weekly - One", "merchant": "Huntress Weekly", "order": 500,
         "objectives": [{"type": "Fetch", "count": 2, "item_id": "Diamond"}], "rewards": []},
        {"id": "Valentine_01", "merchant": "Valentine", "order": 900,
         "objectives": [{"type": "Fetch", "count": 1, "item_id": "Bavin"}], "rewards": []}
    ]});
    QuestCatalog::from_json(&quests.to_string()).expect("quest catalog")
}

pub fn item_catalog() -> ItemCatalog {
    let item = |name: &str, rarity: &str, kind: &str| json!({"name": name, "rarity": rarity, "type": kind, "iconPath": format!("icons/{name}.webp")});
    let items = json!({
        "Bandage_1001": item("Bandage", "Poor", "Utility"),
        "Bandage_2001": item("Bandage", "Common", "Utility"),
        "Diamond_3001": item("Diamond (Normal)", "Uncommon", "Misc"),
        "Diamond_4001": item("Diamond (Exquisite)", "Rare", "Misc"),
        "Diamond_5001": item("Diamond (Perfect)", "Epic", "Misc"),
        "Bavin": item("Bavin", "Common", "Misc"),
        "CrackedLog": item("Cracked Log", "Common", "Misc"),
        "CampfireKit_2001": item("Campfire Kit", "Common", "Utility"),
        "BatWing": item("Bat Wing", "Common", "Misc"),
        "GoldCoins": item("Gold Coins", "Common", "Misc"),
        "SurgicalKit_2001": item("Surgical Kit", "Common", "Utility"),
    });
    ItemCatalog::from_json(&items.to_string()).expect("item catalog")
}

/// A quest as a message shows it, with missions as (content id, count).
pub fn shown(merchant: &str, quest_id: &str, flag: QuestFlag, missions: &[(&str, i32)]) -> CapturedQuest {
    CapturedQuest {
        merchant_id: merchant.to_string(),
        quest_id: quest_id.to_string(),
        quest_flag: flag.to_raw(),
        missions: missions
            .iter()
            .map(|&(content_id, value)| CapturedMission { content_id: content_id.to_string(), current_value: value, completed: false })
            .collect(),
        ..CapturedQuest::default()
    }
}

pub fn owned(unique_id: u64, item_id: &str, count: u32, inventory_id: u32, loot_state: i32) -> OwnedItem {
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

pub fn character(items: Vec<OwnedItem>) -> Character {
    Character { id: "1".into(), name: "Wizard".into(), class: "Wizard".into(), level: 20, items, storages: Vec::new() }
}

pub fn stash_label(inventory_id: u32) -> String {
    format!("Stash {inventory_id}")
}

/// Everything a view needs, owned, so a test can tweak a part and build views from it.
pub struct World {
    pub catalog: QuestCatalog,
    pub items: ItemCatalog,
    pub families: ItemFamilyIndex,
    pub progress: QuestProgress,
    pub tracked: TrackedState,
    pub active: Vec<String>,
    pub characters: Vec<Character>,
}

impl World {
    pub fn new() -> Self {
        let items = item_catalog();
        let families = ItemFamilyIndex::build(&items);
        Self {
            catalog: quest_catalog(),
            items,
            families,
            progress: QuestProgress::default(),
            tracked: TrackedState::default(),
            active: Vec::new(),
            characters: Vec::new(),
        }
    }

    pub fn inputs(&self) -> ViewInputs<'_> {
        ViewInputs {
            catalog: &self.catalog,
            progress: &self.progress,
            tracked: &self.tracked,
            active_merchants: &self.active,
            characters: &self.characters,
            items: &self.items,
            families: &self.families,
            stash_label: &stash_label,
        }
    }
}
