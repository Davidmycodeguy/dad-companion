//! What the Quests page receives: plain serializable views, camelCase for the TypeScript side.

use serde::Serialize;

/// Where a quest stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestState {
    /// Its prerequisite isn't done yet, or the game says it's locked.
    Locked,
    /// Open to take; nothing done yet.
    Available,
    /// Accepted, or some progress recorded.
    Active,
    /// Every objective met: hand it in.
    Ready,
    /// Handed in, or ticked off.
    Done,
}

/// Who says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The game's own messages.
    Game,
    /// The player ticked it (here or in DnDTools).
    Manual,
    /// A later quest of its chain is open, so it must be done.
    Implied,
}

/// An item as the page draws it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    /// The catalog item shown (the required grade, or the family's first); None when unknown.
    pub id: Option<String>,
    pub name: String,
    /// The rarity the slot is lit with ("Unknown" when any rarity will do).
    pub rarity: &'static str,
    pub icon: Option<String>,
}

/// Where some of an item is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub character: String,
    pub stash: String,
    pub usable: u32,
    pub other: u32,
}

/// What the player owns of what a quest asks for, across characters (never the locked seasonal
/// stash, whose items are only a preview).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Owned {
    /// Looted items that fit: what can be handed in.
    pub usable: u32,
    /// Items that fit but weren't looted (bought, crafted, traded): quests don't take them.
    pub other: u32,
    /// Most first.
    pub places: Vec<Place>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveView {
    /// Its position in the quest (what setting progress refers to).
    pub index: usize,
    /// The catalog's type: "Fetch", "Kill", "Explore", "Use Item", "Props", "Survive", "Hold", "Damage".
    pub kind: String,
    /// "Bring 2 Rare Diamond", "Kill 3 Goblin Archer", "Explore Ruins Keep".
    pub label: String,
    pub count: u32,
    /// Handed in, killed or done so far (at most `count`).
    pub submitted: u32,
    pub done: bool,
    pub source: Option<Source>,
    /// The item to bring or use.
    pub item: Option<ItemRef>,
    /// The lowest rarity that counts, when the quest names one.
    pub min_rarity: Option<&'static str>,
    /// What the player owns of it (item objectives only).
    pub owned: Option<Owned>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardView {
    /// The catalog's type: "Item", "Experience", "Affinity", "Random", ...
    pub kind: String,
    /// "Surgical Kit", "Experience", "Alchemist affinity", "Random Rare Armor".
    pub label: String,
    pub count: u32,
    pub item: Option<ItemRef>,
    /// For random rewards: the rarity they come in.
    pub rarity: Option<&'static str>,
    /// Gold coins (the page shows them as gold).
    pub gold: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrerequisiteView {
    pub id: String,
    pub title: String,
    pub merchant: String,
    pub done: bool,
}

/// What the game reported for missions that couldn't be tied to one objective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTally {
    pub submitted: u32,
    pub needed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestView {
    pub id: String,
    pub title: String,
    pub text: Option<String>,
    pub dungeons: Vec<String>,
    pub state: QuestState,
    /// Where the state comes from; None when it follows from the prerequisites alone.
    pub source: Option<Source>,
    /// "Daily", "Weekly" or "Seasonal" for quests that rotate.
    pub time_limit: Option<&'static str>,
    pub prerequisite: Option<PrerequisiteView>,
    pub objectives: Vec<ObjectiveView>,
    pub rewards: Vec<RewardView>,
    pub unmatched: Option<GameTally>,
    /// When the game last showed this quest (Unix seconds).
    pub seen_at: Option<f64>,
}

/// The quest to work on next at a merchant.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentQuest {
    pub id: String,
    pub title: String,
    pub state: QuestState,
    /// For a locked quest: what it waits for.
    pub waits_for: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantSummary {
    /// The merchant's name as the catalog spells it ("Tavern Master").
    pub name: String,
    pub total: u32,
    pub done: u32,
    pub ready: u32,
    pub active: u32,
    pub available: u32,
    pub locked: u32,
    /// The app has read this merchant's quests from the game.
    pub tracked: bool,
    /// A quest is ready to hand in here (the game's flag or a quest's state).
    pub turn_in: bool,
    pub current: Option<CurrentQuest>,
    /// Item objectives here the player has looted items for.
    pub bring: u32,
    /// When the game last showed one of this merchant's quests (Unix seconds).
    pub seen_at: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepAction {
    TurnIn,
    Bring,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepItem {
    pub item: ItemRef,
    /// How many to bring now (owned, looted).
    pub count: u32,
    /// How many the objective still needs.
    pub needed: u32,
    pub min_rarity: Option<&'static str>,
}

/// Something the player can do at a merchant right now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextStep {
    pub merchant: String,
    pub quest_id: String,
    pub quest_title: String,
    pub action: StepAction,
    /// For `Bring`: what to bring (owned, looted, and still needed).
    pub items: Vec<StepItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestsOverview {
    pub merchants: Vec<MerchantSummary>,
    pub next_steps: Vec<NextStep>,
    /// When the game last reported anything about quests (Unix seconds).
    pub last_update: Option<f64>,
    /// Characters whose items are counted.
    pub characters: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantQuests {
    pub merchant: MerchantSummary,
    /// In chain order: each quest after its prerequisite.
    pub quests: Vec<QuestView>,
}

/// One quest that still needs an item.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedQuest {
    pub quest_id: String,
    pub title: String,
    pub merchant: String,
    pub remaining: u32,
    pub state: QuestState,
}

/// An item quests still ask for, over every quest that asks.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemNeed {
    pub key: String,
    /// Used in the dungeon ("Use Item") rather than handed in.
    pub use_in_dungeon: bool,
    pub item: ItemRef,
    pub min_rarity: Option<&'static str>,
    pub owned: Owned,
    pub quests: Vec<NeedQuest>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestItems {
    pub items: Vec<ItemNeed>,
    pub characters: usize,
}
