//! The item catalog: every item's name, rarity, type, size and merchant price, from `assets/items.json`.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use crate::Error;

/// Item rarity, lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Rarity {
    Poor,
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
    Unique,
    Artifact,
    #[default]
    Unknown,
}

impl Rarity {
    /// From the game's name for it ("Epic", "legendary", ...); anything else is `Unknown`.
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "poor" => Rarity::Poor,
            "common" => Rarity::Common,
            "uncommon" => Rarity::Uncommon,
            "rare" => Rarity::Rare,
            "epic" => Rarity::Epic,
            "legendary" | "legend" => Rarity::Legendary,
            "unique" => Rarity::Unique,
            "artifact" => Rarity::Artifact,
            _ => Rarity::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rarity::Poor => "Poor",
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
            Rarity::Epic => "Epic",
            Rarity::Legendary => "Legendary",
            Rarity::Unique => "Unique",
            Rarity::Artifact => "Artifact",
            Rarity::Unknown => "Unknown",
        }
    }
}

/// One item as the rest of the app needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub rarity: Rarity,
    /// "Weapon", "Armor", "Accessory", "Utility", ... (empty for loot)
    pub item_type: String,
    pub slot_type: String,
    pub hand_type: String,
    pub weapon_type: String,
    pub armor_type: String,
    pub utility_type: String,
    pub tradable: bool,
    pub max_stack: u32,
    pub width: u32,
    pub height: u32,
    /// What a merchant pays for one.
    pub vendor_price: u32,
    /// Path inside the icon pack, e.g. "icons/Weapon/HeaterShield_5001.webp".
    pub icon_path: Option<String>,
}

impl Item {
    /// Inventory cells it takes.
    pub fn slots(&self) -> u32 {
        self.width.max(1) * self.height.max(1)
    }

    /// The line under the title: "Shield · One Handed", "Chest · Cloth", "Necklace", "" for loot.
    pub fn kind_line(&self) -> String {
        let parts: Vec<&str> = match self.item_type.as_str() {
            "Weapon" => vec![&self.weapon_type, &self.hand_type],
            "Armor" => vec![&self.slot_type, &self.armor_type],
            "Accessory" => vec![&self.slot_type],
            "Utility" => vec![if self.utility_type.is_empty() { "Utility" } else { &self.utility_type }],
            _ => vec![],
        };
        parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
    }
}

/// Every item by id.
#[derive(Debug, Default)]
pub struct ItemCatalog {
    items: HashMap<String, Item>,
}

impl ItemCatalog {
    pub fn load(path: &Path) -> Result<Self, Error> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn from_json(json: &str) -> Result<Self, Error> {
        let raw: HashMap<String, RawItem> = serde_json::from_str(json)?;
        let items = raw.into_iter().map(|(id, raw)| (id.clone(), raw.into_item(id))).collect();
        Ok(Self { items })
    }

    pub fn get(&self, id: &str) -> Option<&Item> {
        self.items.get(id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.values()
    }

    /// Items whose name contains every word of `query` (any case), grouped by name, best match
    /// first: the exact name, then names starting with the query, then names where every word of
    /// the query starts a word, then the rest; shorter names first within each.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit<'_>> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let phrase = words.join(" ");
        let mut groups: HashMap<&str, Vec<&Item>> = HashMap::new();
        for item in self.items.values().filter(|item| !item.name.is_empty()) {
            groups.entry(item.name.as_str()).or_default().push(item);
        }
        let mut ranked: Vec<(u8, &str)> = groups
            .keys()
            .filter_map(|name| match_rank(name, &phrase, &words).map(|rank| (rank, *name)))
            .collect();
        ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.len().cmp(&b.1.len())).then(a.1.cmp(b.1)));
        ranked
            .into_iter()
            .take(limit)
            .map(|(_, name)| {
                let mut variants = groups.remove(name).unwrap_or_default();
                variants.sort_by(|a, b| a.rarity.cmp(&b.rarity).then_with(|| a.id.cmp(&b.id)));
                SearchHit { name, variants }
            })
            .collect()
    }
}

/// The items sharing one name (its rarity variants), lowest rarity first.
#[derive(Debug)]
pub struct SearchHit<'a> {
    pub name: &'a str,
    pub variants: Vec<&'a Item>,
}

/// How well `name` matches (lower is better), or None when a word of the query is missing.
fn match_rank(name: &str, phrase: &str, words: &[String]) -> Option<u8> {
    let lower = name.to_lowercase();
    if !words.iter().all(|word| lower.contains(word.as_str())) {
        return None;
    }
    if lower == phrase {
        return Some(0);
    }
    if lower.starts_with(phrase) {
        return Some(1);
    }
    let name_words: Vec<&str> = lower.split_whitespace().collect();
    if words.iter().all(|word| name_words.iter().any(|w| w.starts_with(word.as_str()))) {
        return Some(2);
    }
    Some(3)
}

/// items.json as written by the asset pipeline (many more fields; only these are read).
#[derive(Deserialize)]
struct RawItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    rarity: String,
    #[serde(default, rename = "type")]
    item_type: String,
    #[serde(default)]
    slot_type: String,
    #[serde(default)]
    hand_type: String,
    #[serde(default)]
    weapon_type: String,
    #[serde(default)]
    armor_type: String,
    #[serde(default)]
    utility_type: String,
    #[serde(default)]
    is_tradable: Option<bool>,
    #[serde(default)]
    max_stack_size: Option<u32>,
    #[serde(default)]
    inventory_width: Option<u32>,
    #[serde(default)]
    inventory_height: Option<u32>,
    #[serde(default)]
    vendor_price: Option<u32>,
    #[serde(default, rename = "iconPath")]
    icon_path: Option<String>,
}

impl RawItem {
    fn into_item(self, id: String) -> Item {
        Item {
            id,
            name: self.name,
            rarity: Rarity::from_name(&self.rarity),
            item_type: self.item_type,
            slot_type: self.slot_type,
            hand_type: self.hand_type,
            weapon_type: self.weapon_type,
            armor_type: self.armor_type,
            utility_type: self.utility_type,
            tradable: self.is_tradable.unwrap_or(true),
            max_stack: self.max_stack_size.unwrap_or(1).max(1),
            width: self.inventory_width.unwrap_or(1),
            height: self.inventory_height.unwrap_or(1),
            vendor_price: self.vendor_price.unwrap_or(0),
            icon_path: self.icon_path.filter(|p| !p.is_empty()),
        }
    }
}
