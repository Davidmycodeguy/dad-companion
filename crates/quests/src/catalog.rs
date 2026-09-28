//! The quest catalog: static quest definitions loaded from the bundled `assets/quests.json`
//! snapshot (DarkerDB v2 shape, already normalized by the asset pipeline — see
//! `normalize_darkerdb_v2_quest` in the Python `quest_service.py` for the source transform).
//!
//! Unlike the Python service, this crate never re-fetches the catalog from DarkerDB at runtime: the
//! task this crate serves ("follow quests from the game's lobby messages") only needs the static
//! definitions, so the network fetch/pagination/cache-staleness machinery in `quest_service.py` was
//! deliberately not ported. Loading always reads a snapshot already produced by the asset pipeline.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::Error;

/// One quest: its catalog metadata, what it asks for, and what it pays out.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Quest {
    pub id: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub chapter: Option<String>,
    #[serde(default)]
    pub chapter_id: Option<String>,
    /// The quest id that must be completed first, if any. In the DarkerDB v2 snapshot this is
    /// already a canonical quest id (equal to some other quest's `id`), not free text — unlike the
    /// browser UI's alias-matching resolver, so a direct lookup by id is all that's needed here.
    #[serde(default)]
    pub prerequisite: Option<String>,
    #[serde(default)]
    pub dungeons: Vec<String>,
    #[serde(default)]
    pub merchant: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub completion_text: Option<String>,
    #[serde(default)]
    pub objectives: Vec<Objective>,
    #[serde(default)]
    pub rewards: Vec<Reward>,
    #[serde(default)]
    pub order: Option<u32>,
    #[serde(default)]
    pub is_repeatable: bool,
    #[serde(default)]
    pub is_daily: bool,
    #[serde(default)]
    pub patch: Option<String>,
    #[serde(default)]
    pub season: Option<String>,
}

/// Something the player must do to progress a quest (a "Fetch", "Kill", "Explore", "Use Item",
/// "Props", "Survive", "Hold" or "Damage" entry, per the current snapshot).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Objective {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub count: Option<u32>,
    /// The item archetype (e.g. "Bandage"), for Fetch/Use Item objectives.
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    /// The monster display name (e.g. "Skeleton Champion"), for Kill objectives.
    #[serde(default)]
    pub monster: Option<String>,
    #[serde(default)]
    pub module: Option<String>,
    /// What to interact with, for Props objectives.
    #[serde(default)]
    pub interact: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub item_type: Option<String>,
}

/// Something the player receives for completing a quest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Reward {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    /// The merchant whose affinity this reward raises, for "Affinity" rewards.
    #[serde(default)]
    pub merchant: Option<String>,
    #[serde(default)]
    pub item_type: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub random_reward_id: Option<String>,
    #[serde(default)]
    pub item_skin_id: Option<String>,
    #[serde(default)]
    pub emote_id: Option<String>,
    #[serde(default)]
    pub action_skin_id: Option<String>,
}

/// The bundled snapshot's outer shape; only `quests` matters here, the rest (`version`, `source`,
/// `timestamp`, ...) is metadata for the asset pipeline that this crate doesn't need to act on.
#[derive(Deserialize)]
struct Snapshot {
    #[serde(default)]
    quests: Vec<Quest>,
}

/// Every quest, indexed by id, in the order the snapshot listed them.
#[derive(Debug, Default)]
pub struct QuestCatalog {
    quests: Vec<Quest>,
    by_id: HashMap<String, usize>,
}

impl QuestCatalog {
    pub fn load(path: &Path) -> Result<Self, Error> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn from_json(json: &str) -> Result<Self, Error> {
        let snapshot: Snapshot = serde_json::from_str(json)?;
        let by_id = snapshot
            .quests
            .iter()
            .enumerate()
            .map(|(index, quest)| (quest.id.clone(), index))
            .collect();
        Ok(Self { quests: snapshot.quests, by_id })
    }

    pub fn get(&self, id: &str) -> Option<&Quest> {
        self.by_id.get(id).map(|&index| &self.quests[index])
    }

    pub fn len(&self) -> usize {
        self.quests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quests.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Quest> {
        self.quests.iter()
    }

    /// Quests offered by `merchant` (matched case-sensitively, as stored in the snapshot).
    pub fn for_merchant<'a>(&'a self, merchant: &'a str) -> impl Iterator<Item = &'a Quest> {
        self.quests.iter().filter(move |quest| quest.merchant == merchant)
    }
}
