//! Everything the Quests page reads and the game's merchant messages update, behind one type the
//! app shares between its network thread (the `on_*` methods) and its commands (the views and the
//! player's own ticks). What the game showed is saved after every message, so quest status
//! survives a restart.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use game_data::ItemCatalog;
use state::Character;

use crate::catalog::{Objective, QuestCatalog};
use crate::packet_handler::{QuestEvent, QuestPacketHandler};
use crate::packet_types::{
    MerchantListMessage, QuestCompleteMessage, QuestContentValueStackMessage, QuestListMessage, QuestLogMessage,
    QuestSelectMessage,
};
use crate::progress::{ObjectiveProgress, QuestProgress};
use crate::service::QuestService;
use crate::text::normalize_game_id;
use crate::tracked::TrackedState;
use crate::view::{self, MerchantQuests, QuestItems, QuestsOverview, ViewInputs};
use crate::Error;

/// Progress keys packet capture writes; everything else is the player's own.
const CAPTURED_PREFIX: &str = "captured::";

pub struct QuestTracker {
    service: QuestService,
    handler: Mutex<QuestPacketHandler>,
}

impl QuestTracker {
    /// Opens the quest catalog at `quests_json` and the progress kept in `data_dir`, picking up
    /// what the game showed before the last restart. `items` names and groups quest items.
    pub fn open(quests_json: &Path, data_dir: &Path, items: &ItemCatalog) -> Result<Self, Error> {
        let service = QuestService::new(QuestCatalog::load(quests_json)?, items, data_dir);
        let mut handler = QuestPacketHandler::new();
        // A damaged or older save only loses what the game showed; progress itself is kept.
        if let (Some(saved), _) = service.load_captured_state() {
            if let Ok(tracked) = serde_json::from_value::<TrackedState>(saved) {
                handler.restore_tracked(tracked);
            }
        }
        Ok(Self { service, handler: Mutex::new(handler) })
    }

    pub fn service(&self) -> &QuestService {
        &self.service
    }

    /// The game's merchant list: which merchants it offers now, and their flags. Returns whether
    /// anything the page shows changed.
    pub fn on_merchant_list(&self, message: &MerchantListMessage) -> Result<bool, Error> {
        let (events, tracked) = {
            let mut handler = self.lock();
            let events = handler.handle_merchant_list(message);
            (events, handler.tracked().clone())
        };
        let mut merchants: Vec<String> = Vec::new();
        for id in message.merchant_list.iter().map(|m| normalize_game_id(&m.merchant_id)).filter(|id| !id.is_empty()) {
            if !merchants.contains(&id) {
                merchants.push(id);
            }
        }
        let merchants_changed = !merchants.is_empty() && merchants != self.service.load_active_merchants();
        if merchants_changed {
            self.service.save_active_merchants(&merchants)?;
        }
        if !events.is_empty() {
            self.save(&tracked)?;
        }
        Ok(!events.is_empty() || merchants_changed)
    }

    /// A merchant's quest list (its window opened).
    pub fn on_quest_list(&self, message: &QuestListMessage) -> Result<bool, Error> {
        self.record(|handler, service| handler.handle_quest_list(message, service.progress_store()))
    }

    /// The full quest log.
    pub fn on_quest_log(&self, message: &QuestLogMessage) -> Result<bool, Error> {
        self.record(|handler, service| handler.handle_quest_log(message, service.progress_store()))
    }

    /// A quest was turned in.
    pub fn on_quest_complete(&self, message: &QuestCompleteMessage) -> Result<bool, Error> {
        self.record(|handler, service| handler.handle_quest_complete(message, service.progress_store()))
    }

    /// A quest was accepted; the game sends the updated list after it.
    pub fn on_quest_select(&self, message: &QuestSelectMessage) -> bool {
        !self.lock().handle_quest_select(message).is_empty()
    }

    /// Items were handed in toward a quest; the game sends the updated list after it.
    pub fn on_content_value_stack(&self, message: &QuestContentValueStackMessage) -> bool {
        !self.lock().handle_quest_content_value_stack(message).is_empty()
    }

    fn record(
        &self,
        handle: impl FnOnce(&mut QuestPacketHandler, &QuestService) -> Result<Vec<QuestEvent>, Error>,
    ) -> Result<bool, Error> {
        let (events, tracked) = {
            let mut handler = self.lock();
            let events = handle(&mut handler, &self.service)?;
            (events, handler.tracked().clone())
        };
        self.save(&tracked)?;
        Ok(!events.is_empty())
    }

    fn save(&self, tracked: &TrackedState) -> Result<(), Error> {
        self.service.save_captured_state(&serde_json::to_value(tracked)?)
    }

    fn lock(&self) -> MutexGuard<'_, QuestPacketHandler> {
        // A panic mid-message leaves the handler usable: at worst one message is half applied.
        self.handler.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Views and the player's own progress.
impl QuestTracker {
    pub fn overview(&self, characters: &[Character], items: &ItemCatalog, stash_label: &dyn Fn(u32) -> String) -> QuestsOverview {
        self.with_inputs(characters, items, stash_label, view::overview)
    }

    pub fn merchant_quests(
        &self,
        merchant: &str,
        characters: &[Character],
        items: &ItemCatalog,
        stash_label: &dyn Fn(u32) -> String,
    ) -> Option<MerchantQuests> {
        self.with_inputs(characters, items, stash_label, |inputs| view::merchant_quests(inputs, merchant))
    }

    pub fn quest_items(&self, characters: &[Character], items: &ItemCatalog, stash_label: &dyn Fn(u32) -> String) -> QuestItems {
        self.with_inputs(characters, items, stash_label, view::quest_items)
    }

    fn with_inputs<T>(
        &self,
        characters: &[Character],
        items: &ItemCatalog,
        stash_label: &dyn Fn(u32) -> String,
        build: impl FnOnce(&ViewInputs<'_>) -> T,
    ) -> T {
        let (progress, _timestamp, _revision, active_merchants) = self.service.load_progress_sync_state();
        let tracked = self.lock().tracked().clone();
        build(&ViewInputs {
            catalog: self.service.catalog(),
            progress: &progress,
            tracked: &tracked,
            active_merchants: &active_merchants,
            characters,
            items,
            families: self.service.item_families(),
            stash_label,
        })
    }

    /// The player's own progress on one objective: how many are handed in (None: all when `done`,
    /// else what was recorded, below the count) and whether it's done.
    pub fn set_objective(&self, quest_id: &str, index: usize, submitted: Option<u32>, done: bool) -> Result<(), Error> {
        let quest = self.service.quest(quest_id).ok_or_else(|| Error::UnknownQuest(quest_id.to_string()))?;
        let objective =
            quest.objectives.get(index).ok_or_else(|| Error::UnknownQuest(format!("{quest_id} objective {}", index + 1)))?;
        let key = manual_key(quest_id, index, objective);
        let count = objective.count.unwrap_or(0);
        self.service.update_progress(|mut progress| {
            let previous = progress.objectives.get(&key).map_or(0, |entry| entry.submitted);
            let submitted = match submitted {
                Some(value) => value,
                None if done => count,
                None => previous.min(count.saturating_sub(1)),
            };
            let submitted = if count > 0 { submitted.min(count) } else { submitted };
            if !done && submitted == 0 {
                progress.objectives.remove(&key);
            } else {
                progress.objectives.insert(key, manual_entry(quest_id, index, objective, submitted, done));
            }
            progress
        })?;
        Ok(())
    }

    /// Ticks every objective of a quest done, or clears the player's own progress on it.
    pub fn set_quest_done(&self, quest_id: &str, done: bool) -> Result<(), Error> {
        let quest = self.service.quest(quest_id).ok_or_else(|| Error::UnknownQuest(quest_id.to_string()))?;
        self.service.update_progress(|mut progress: QuestProgress| {
            progress.objectives.retain(|key, entry| key.starts_with(CAPTURED_PREFIX) || !is_about(key, entry, quest_id));
            if done {
                for (index, objective) in quest.objectives.iter().enumerate() {
                    let count = objective.count.unwrap_or(0);
                    let key = manual_key(quest_id, index, objective);
                    progress.objectives.insert(key, manual_entry(quest_id, index, objective, count, true));
                }
            }
            progress
        })?;
        Ok(())
    }
}

/// The key DnDTools used for the player's own progress: `<quest>::<type>::<index>::<target>`.
fn manual_key(quest_id: &str, index: usize, objective: &Objective) -> String {
    let kind = if objective.kind.is_empty() { "Objective" } else { objective.kind.as_str() };
    let target = [&objective.item_id, &objective.monster, &objective.module, &objective.interact]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty());
    match target {
        Some(target) => format!("{quest_id}::{kind}::{index}::{target}"),
        None => format!("{quest_id}::{kind}::{index}"),
    }
}

fn manual_entry(quest_id: &str, index: usize, objective: &Objective, submitted: u32, completed: bool) -> ObjectiveProgress {
    ObjectiveProgress {
        quest_id: Some(quest_id.to_string()),
        objective_index: i64::try_from(index).ok(),
        kind: Some(objective.kind.clone()),
        item_id: objective.item_id.clone(),
        submitted,
        completed,
    }
}

/// Whether a manual progress entry is about `quest_id` (its own field, else its key).
fn is_about(key: &str, entry: &ObjectiveProgress, quest_id: &str) -> bool {
    match &entry.quest_id {
        Some(id) => id == quest_id,
        None => key.split("::").next() == Some(quest_id),
    }
}
