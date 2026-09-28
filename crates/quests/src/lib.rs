//! Merchant quest tracking, ported from DnDTools' `quest_service.py` and `quest_packet_handler.py`.
//!
//! The app follows the player's merchant quests from the game's lobby messages: quest lists, the
//! quest log, selecting and completing quests, and submitted item counts. This crate holds the
//! quest catalog (loaded from the bundled `assets/quests.json` snapshot), per-character progress
//! (persisted atomically to a JSON file in a folder the app chooses), the packet handler that turns
//! decoded lobby messages into progress updates and UI events, and the dependency-order/matching
//! helpers the quest list view needs.
//!
//! On top of that port: [`tracked`] keeps the newest state the game showed of each quest (saved so
//! it survives a restart), [`mission_match`] ties the game's missions to catalog objectives by name,
//! [`view`] builds the Quests page's views as pure functions, [`tracker::QuestTracker`] puts it all
//! behind one type the app shares between its network thread and its commands, and [`import`] takes
//! over DnDTools' saved progress once.
//!
//! This crate never depends on the `protocol` crate's generated protobuf types. Instead,
//! [`packet_types`] defines plain structs that mirror exactly the fields the Python handlers read
//! from each decoded message; the app converts real decoded messages into them.

pub mod captured_state;
pub mod catalog;
mod fs_util;
pub mod import;
pub mod items;
pub mod loot;
pub mod mission_match;
pub mod packet_handler;
pub mod packet_types;
pub mod progress;
pub mod service;
pub mod sorting;
pub mod text;
pub mod tracked;
pub mod tracker;
pub mod view;

// Re-exports are added module by module as each one is implemented.
pub use catalog::{Objective, Quest, QuestCatalog, Reward};
pub use items::{
    build_item_payload, merge_item_family_holdings, owned_count, CharacterHolding, IconUrlBuilder, ItemFamily,
    ItemFamilyIndex, ItemInfo, ItemPayload, StashHolding,
};
pub use captured_state::CapturedStateStore;
pub use mission_match::match_missions;
pub use packet_handler::{
    AutoProgress, CapturedChapter, CapturedMission, CapturedQuest, CapturedQuestLogEntry, CapturedState,
    CompletionReward, QuestCompletion, QuestEvent, QuestPacketHandler,
};
pub use packet_types::{
    MerchantFlag, MerchantInfoInput, MerchantListMessage, QuestChapterInfoInput, QuestCompleteMessage,
    QuestContentInfoInput, QuestContentValueStackMessage, QuestFlag, QuestInfoInput, QuestListMessage,
    QuestLogEntryInput, QuestLogMessage, QuestSelectMessage, RewardInfoInput,
};
pub use progress::{ObjectiveProgress, ProgressStore, QuestProgress};
pub use service::QuestService;
pub use sorting::{compute_quest_display_order, ObjectiveDescriptor, ObjectiveMatcher, QuestOrderInput};
pub use tracked::{TrackedQuest, TrackedState};
pub use tracker::QuestTracker;

/// Everything that can go wrong loading or persisting quest data.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read quest data: {0}")]
    Io(#[from] std::io::Error),
    #[error("quest data is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no quest {0}")]
    UnknownQuest(String),
}
