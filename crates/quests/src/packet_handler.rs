//! Turns decoded merchant/quest lobby messages into captured quest state and UI events. Ported
//! from `quest_packet_handler.py`'s `QuestPacketHandler`.
//!
//! Python's handlers return a callback-driven `bool` (success, with a `ui_notify(event, json)`
//! side-channel for notifications) and hold a reference to a `QuestService` for progress
//! persistence. This port instead returns the UI events directly as data (easier to test and to
//! route from the app, and no callback needed at all), and takes the [`crate::ProgressStore`] to
//! sync into as an explicit parameter on the three handlers that persist progress — rather than
//! this type owning one — so callers choose the lifetime/threading model.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::packet_types::{
    MerchantListMessage, QuestChapterInfoInput, QuestCompleteMessage, QuestContentValueStackMessage, QuestFlag,
    QuestInfoInput, QuestListMessage, QuestLogMessage, QuestSelectMessage,
};
use crate::progress::{ObjectiveProgress, ProgressStore, QuestProgress};
use crate::text::normalize_game_id;
use crate::tracked::TrackedState;
use crate::Error;

/// `handle_quest_list` (`SS2C_MERCHANT_QUEST_LIST_INFO_RES`) never carries a merchant id — see the
/// doc comment on [`QuestPacketHandler::handle_quest_list`] — so its snapshots always live under
/// this synthetic key, exactly as `quest_packet_handler.py` stores them.
const LAST_MERCHANT_KEY: &str = "_last_merchant";

/// How many recent completions `captured_state`/`auto_progress` include (Python's `[-20:]`).
const RECENT_COMPLETIONS_WINDOW: usize = 20;
/// How many completions are kept in memory at all before the oldest are dropped (Python's `[-50:]`).
const MAX_STORED_COMPLETIONS: usize = 50;
/// The synthetic objective "type" packet-derived progress is tagged with, distinguishing it from
/// browser-submitted progress in the same `QuestProgress.objectives` map.
const CAPTURED_OBJECTIVE_KIND: &str = "captured";

/// One submitted-item counter captured from a quest's `missions` list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CapturedMission {
    pub content_id: String,
    pub current_value: i32,
    /// Always `false` when freshly parsed; set `true` for every mission of a quest that
    /// [`QuestPacketHandler::handle_quest_complete`] later completes.
    pub completed: bool,
}

/// One quest's state as captured from a lobby message.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CapturedQuest {
    /// Empty for entries captured via `handle_quest_list` (see its doc comment); populated for
    /// entries captured via `handle_quest_log`.
    pub merchant_id: String,
    pub quest_id: String,
    pub quest_order: u32,
    pub chapter_id: String,
    pub quest_flag: i32,
    pub already_get_affinity: i32,
    pub missions: Vec<CapturedMission>,
}

impl CapturedQuest {
    pub fn quest_flag(&self) -> QuestFlag {
        QuestFlag::from_raw(self.quest_flag)
    }
}

/// A chapter's remaining time, captured alongside a merchant's quest list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct CapturedChapter {
    pub chapter_id: String,
    pub remain_ms_time: u64,
}

/// One merchant's section of a full quest-log snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct CapturedQuestLogEntry {
    pub merchant_id: String,
    pub quests: Vec<CapturedQuest>,
    pub chapters: Vec<CapturedChapter>,
}

/// One reward line from a quest-complete response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompletionReward {
    pub reward_type: String,
    pub stock_id: String,
    pub reward_count: u32,
}

/// A recorded quest completion.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuestCompletion {
    pub result: i32,
    pub merchant_id: String,
    pub quest_id: String,
    pub chapter_id: String,
    pub rewards: Vec<CompletionReward>,
    pub timestamp: f64,
}

/// A snapshot of everything captured so far, for the UI. Mirrors `get_captured_state`.
#[derive(Debug, Clone, Default)]
pub struct CapturedState {
    pub merchant_quests: HashMap<String, Vec<CapturedQuest>>,
    pub merchant_chapters: HashMap<String, Vec<CapturedChapter>>,
    pub merchant_flags: HashMap<String, u32>,
    pub quest_log: Option<Vec<CapturedQuestLogEntry>>,
    pub recent_completions: Vec<QuestCompletion>,
    pub last_update: f64,
}

/// One quest per id, deduplicated from the full log when available or every per-merchant capture
/// otherwise. Mirrors `get_auto_progress`/`_build_auto_progress`.
#[derive(Debug, Clone, Default)]
pub struct AutoProgress {
    pub quests: HashMap<String, CapturedQuest>,
    pub completions: Vec<QuestCompletion>,
    pub merchant_flags: HashMap<String, u32>,
    pub last_update: f64,
}

/// A UI notification a handler produced. Replaces Python's `ui_notify(event_name, json_payload)`
/// callback with plain data the app can route however it likes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestEvent {
    /// A merchant list arrived with flags different from the last one seen.
    MerchantListUpdated { merchant_count: usize },
    /// A single merchant's quest list changed.
    QuestListUpdated { quest_count: usize, in_progress: usize },
    /// The full quest log changed.
    QuestLogUpdated { quest_count: usize, in_progress: usize },
    /// The player interacted with a quest board (always emitted; a result of `0` is success).
    QuestAccepted { result: i32 },
    /// A quest was turned in.
    QuestCompleted { quest_id: String, merchant_id: String, reward_count: usize },
    /// Items were submitted toward a quest (always emitted).
    QuestItemsSubmitted { result: i32 },
}

/// Tracks quest state captured from lobby messages and turns each message into progress updates and
/// UI events. Not internally synchronized (unlike the Python, which wraps every method in a lock) —
/// wrap it in a `Mutex` if it's shared across threads, the same way callers already must for any
/// other `&mut self` state.
#[derive(Debug, Default)]
pub struct QuestPacketHandler {
    merchant_quests: HashMap<String, Vec<CapturedQuest>>,
    merchant_chapters: HashMap<String, Vec<CapturedChapter>>,
    merchant_flags: HashMap<String, u32>,
    quest_completions: Vec<QuestCompletion>,
    captured_quest_log: Option<Vec<CapturedQuestLogEntry>>,
    last_update: f64,
    data_hashes: HashMap<String, u64>,
    /// The newest state of every quest, whichever message said it (see [`crate::tracked`]).
    tracked: TrackedState,
}

impl QuestPacketHandler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The newest state of every quest the game has shown, for the UI and for saving.
    pub fn tracked(&self) -> &TrackedState {
        &self.tracked
    }

    /// Picks up where a previous run left off (the state [`Self::tracked`] returned then). The
    /// first message after this always reports a change, so the UI refreshes.
    pub fn restore_tracked(&mut self, tracked: TrackedState) {
        self.merchant_flags = tracked.merchant_flags.iter().map(|(id, &flag)| (id.clone(), flag)).collect();
        self.last_update = tracked.last_update;
        self.tracked = tracked;
    }

    /// The latest captured quest state, for the UI. Mirrors `get_captured_state`.
    pub fn captured_state(&self) -> CapturedState {
        CapturedState {
            merchant_quests: self.merchant_quests.clone(),
            merchant_chapters: self.merchant_chapters.clone(),
            merchant_flags: self.merchant_flags.clone(),
            quest_log: self.captured_quest_log.clone(),
            recent_completions: recent_slice(&self.quest_completions, RECENT_COMPLETIONS_WINDOW),
            last_update: self.last_update,
        }
    }

    /// A progress payload built from captured packet data. Mirrors `get_auto_progress`.
    pub fn auto_progress(&self) -> AutoProgress {
        self.build_auto_progress()
    }

    /// Reset all captured state. Mirrors `clear`.
    pub fn clear(&mut self) {
        self.merchant_quests.clear();
        self.merchant_chapters.clear();
        self.merchant_flags.clear();
        self.quest_completions.clear();
        self.captured_quest_log = None;
        self.data_hashes.clear();
        self.last_update = 0.0;
        self.tracked = TrackedState::default();
    }

    /// `SS2C_MERCHANT_LIST_RES` — merchant flags (quest ready, success, etc). Mirrors
    /// `handle_merchant_list`.
    pub fn handle_merchant_list(&mut self, message: &MerchantListMessage) -> Vec<QuestEvent> {
        for merchant in &message.merchant_list {
            let merchant_id = normalize_game_id(&merchant.merchant_id);
            if merchant_id.is_empty() {
                continue;
            }
            self.tracked.record_merchant_flag(&merchant_id, merchant.merchant_flag, now_seconds());
            self.merchant_flags.insert(merchant_id, merchant.merchant_flag);
        }
        self.touch();

        // Sorted, so the digest doesn't depend on the hash map's iteration order.
        let flags_snapshot: BTreeMap<String, u32> = self.merchant_flags.iter().map(|(id, &flag)| (id.clone(), flag)).collect();
        if self.has_data_changed("merchant_flags", &flags_snapshot) {
            vec![QuestEvent::MerchantListUpdated { merchant_count: message.merchant_list.len() }]
        } else {
            Vec::new()
        }
    }

    /// `SS2C_MERCHANT_QUEST_SELECT_RES` — quest acceptance confirmation. Always emitted: a response
    /// here means the player interacted with the quest board, whether or not it succeeded. Mirrors
    /// `handle_quest_select`.
    pub fn handle_quest_select(&mut self, message: &QuestSelectMessage) -> Vec<QuestEvent> {
        vec![QuestEvent::QuestAccepted { result: message.result }]
    }

    /// `SS2C_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES` — item turn-in confirmation. Always emitted, for
    /// the same reason as [`Self::handle_quest_select`]. Mirrors `handle_quest_content_value_stack`.
    pub fn handle_quest_content_value_stack(&mut self, message: &QuestContentValueStackMessage) -> Vec<QuestEvent> {
        vec![QuestEvent::QuestItemsSubmitted { result: message.result }]
    }

    /// `SS2C_MERCHANT_QUEST_LIST_INFO_RES` — quests for a merchant, plus their chapters.
    ///
    /// This response never carries a merchant id (see `protos/Merchant.proto`), so — exactly as
    /// `quest_packet_handler.py` does — this always stores the snapshot under the synthetic
    /// `"_last_merchant"` key rather than a real one; only [`Self::handle_quest_log`] (whose entries
    /// each carry their own `merchantId`) populates real per-merchant keys. Persists progress via
    /// `progress` (see the type-level doc comment) before returning any event.
    pub fn handle_quest_list(
        &mut self,
        message: &QuestListMessage,
        progress: &ProgressStore,
    ) -> Result<Vec<QuestEvent>, Error> {
        let quest_entries: Vec<CapturedQuest> =
            message.quests.iter().filter_map(|raw| parse_quest_info(raw, "")).collect();
        let chapter_entries: Vec<CapturedChapter> = message.chapters.iter().map(parse_chapter_info).collect();

        self.merchant_quests.insert(LAST_MERCHANT_KEY.to_string(), quest_entries.clone());
        self.merchant_chapters.insert(LAST_MERCHANT_KEY.to_string(), chapter_entries);
        self.touch();
        self.tracked.record_quests(&quest_entries, self.last_update);
        self.sync_progress_to_service(progress)?;

        let in_progress = quest_entries.iter().filter(|q| q.quest_flag() == QuestFlag::Progress).count();
        let hash_key = format!("quest_list:{LAST_MERCHANT_KEY}");
        let events = if self.has_data_changed(&hash_key, &quest_entries) {
            vec![QuestEvent::QuestListUpdated { quest_count: quest_entries.len(), in_progress }]
        } else {
            Vec::new()
        };
        Ok(events)
    }

    /// `SS2C_MERCHANT_QUEST_LOG_LIST_RES` — the full quest log across every merchant. Persists
    /// progress via `progress` before returning any event. Mirrors `handle_quest_log`.
    pub fn handle_quest_log(
        &mut self,
        message: &QuestLogMessage,
        progress: &ProgressStore,
    ) -> Result<Vec<QuestEvent>, Error> {
        let mut all_entries = Vec::with_capacity(message.quest_list.len());
        for log_entry in &message.quest_list {
            let merchant_id = normalize_game_id(&log_entry.merchant_id);
            let quests: Vec<CapturedQuest> =
                log_entry.quests.iter().filter_map(|raw| parse_quest_info(raw, &merchant_id)).collect();
            let chapters: Vec<CapturedChapter> = log_entry.chapters.iter().map(parse_chapter_info).collect();

            self.merchant_quests.insert(merchant_id.clone(), quests.clone());
            self.merchant_chapters.insert(merchant_id.clone(), chapters.clone());
            all_entries.push(CapturedQuestLogEntry { merchant_id, quests, chapters });
        }

        self.captured_quest_log = Some(all_entries.clone());
        self.touch();
        let logged: Vec<CapturedQuest> = all_entries.iter().flat_map(|entry| entry.quests.iter().cloned()).collect();
        self.tracked.record_quests(&logged, self.last_update);
        self.sync_progress_to_service(progress)?;

        let total_quests: usize = all_entries.iter().map(|entry| entry.quests.len()).sum();
        let in_progress = all_entries
            .iter()
            .flat_map(|entry| &entry.quests)
            .filter(|quest| quest.quest_flag() == QuestFlag::Progress)
            .count();
        let events = if self.has_data_changed("quest_log", &all_entries) {
            vec![QuestEvent::QuestLogUpdated { quest_count: total_quests, in_progress }]
        } else {
            Vec::new()
        };
        Ok(events)
    }

    /// `SS2C_MERCHANT_QUEST_COMPLETE_RES` — quest turned in with rewards. Marks the matching quest
    /// (if currently tracked) complete, records the completion, and persists progress via
    /// `progress`. Always emits an event. Mirrors `handle_quest_complete`.
    pub fn handle_quest_complete(
        &mut self,
        message: &QuestCompleteMessage,
        progress: &ProgressStore,
    ) -> Result<Vec<QuestEvent>, Error> {
        let merchant_id = normalize_game_id(&message.given_merchant_id);
        let quest_id = normalize_game_id(&message.given_quest_id);
        let chapter_id = normalize_game_id(&message.given_chapter_id);
        let rewards: Vec<CompletionReward> = message
            .rewards
            .iter()
            .map(|reward| CompletionReward {
                reward_type: reward.reward_type.clone(),
                stock_id: reward.stock_id.clone(),
                reward_count: reward.reward_count,
            })
            .collect();
        let reward_count = rewards.len();

        self.quest_completions.push(QuestCompletion {
            result: message.result,
            merchant_id: merchant_id.clone(),
            quest_id: quest_id.clone(),
            chapter_id: chapter_id.clone(),
            rewards,
            timestamp: now_seconds(),
        });
        if self.quest_completions.len() > MAX_STORED_COMPLETIONS {
            let excess = self.quest_completions.len() - MAX_STORED_COMPLETIONS;
            self.quest_completions.drain(0..excess);
        }

        if !merchant_id.is_empty() {
            if let Some(quests) = self.merchant_quests.get_mut(&merchant_id) {
                if let Some(found) = quests.iter_mut().find(|q| q.quest_id == quest_id) {
                    found.quest_flag = QuestFlag::Complete.to_raw();
                    for mission in &mut found.missions {
                        mission.completed = true;
                    }
                }
            }
        }
        self.touch();
        self.tracked.record_completion(&merchant_id, &quest_id, &chapter_id, self.last_update);
        self.sync_progress_to_service(progress)?;

        Ok(vec![QuestEvent::QuestCompleted { quest_id, merchant_id, reward_count }])
    }

    /// One progress-friendly snapshot of every captured quest, by id. Mirrors
    /// `_build_auto_progress`, except that each quest comes from the newest message that showed it:
    /// the Python preferred the full log whenever it had one, so merchant quest lists and turn-ins
    /// seen after the log never reached progress.
    fn build_auto_progress(&self) -> AutoProgress {
        let quests: HashMap<String, CapturedQuest> =
            self.tracked.captured_quests().map(|quest| (quest.quest_id.clone(), quest.clone())).collect();
        AutoProgress {
            quests,
            completions: recent_slice(&self.quest_completions, RECENT_COMPLETIONS_WINDOW),
            merchant_flags: self.merchant_flags.clone(),
            last_update: self.last_update,
        }
    }

    /// Merge captured progress into `progress`'s on-disk state, tagging each submitted-item key as
    /// `"captured::<quest>::<mission index>::<content id>"` so it can't collide with browser-sync
    /// objective keys. A no-op when nothing has been captured yet. Mirrors `_sync_progress_to_service`.
    fn sync_progress_to_service(&self, progress: &ProgressStore) -> Result<(), Error> {
        let auto_progress = self.build_auto_progress();
        if auto_progress.quests.is_empty() {
            return Ok(());
        }
        progress.update_progress(|current| merge_captured_progress(current, &auto_progress.quests))?;
        Ok(())
    }

    fn touch(&mut self) {
        self.last_update = now_seconds();
    }

    /// `true` (and remembers the new digest) the first time `data` is seen under `key`, or whenever
    /// it differs from what was last seen. Python hashes with SHA-256 for the same purpose; a fast
    /// non-cryptographic hash is enough since the digest never leaves this process. Mirrors
    /// `_has_data_changed`.
    fn has_data_changed<T: serde::Serialize>(&mut self, key: &str, data: &T) -> bool {
        let digest = content_digest(data);
        let changed = self.data_hashes.get(key) != Some(&digest);
        if changed {
            self.data_hashes.insert(key.to_string(), digest);
        }
        changed
    }
}

/// Parse one `SMERCHANT_QUEST_INFO` into captured-quest shape, normalizing every game id.
/// `merchant_id` is the caller's already-known merchant for this quest (or `""` when unknown — see
/// [`QuestPacketHandler::handle_quest_list`]'s doc comment); it is normalized here the same way
/// Python's `_parse_quest_info(raw, merchant_id=...)` does. Returns `None` for an entry with no
/// quest id, which can't be tracked.
fn parse_quest_info(raw: &QuestInfoInput, merchant_id: &str) -> Option<CapturedQuest> {
    if raw.quest_id.is_empty() {
        return None;
    }
    let normalized_merchant = if merchant_id.is_empty() { String::new() } else { normalize_game_id(merchant_id) };
    let missions = raw
        .missions
        .iter()
        .map(|mission| CapturedMission {
            content_id: normalize_game_id(&mission.content_id),
            current_value: mission.content_current_value,
            completed: false,
        })
        .collect();

    Some(CapturedQuest {
        merchant_id: normalized_merchant,
        quest_id: normalize_game_id(&raw.quest_id),
        quest_order: raw.quest_order,
        chapter_id: normalize_game_id(&raw.chapter_id),
        quest_flag: raw.quest_flag,
        already_get_affinity: raw.already_get_affinity,
        missions,
    })
}

fn parse_chapter_info(raw: &QuestChapterInfoInput) -> CapturedChapter {
    CapturedChapter { chapter_id: normalize_game_id(&raw.chapter_id), remain_ms_time: raw.remain_ms_time }
}

/// Fold captured mission progress into `existing`'s objectives, keyed so it can never collide with a
/// browser-submitted objective key. Submitted counts only ever move up (`max` of what's stored and
/// what was just captured) and `completed` latches once true, so a stale or partial re-capture can
/// never erase progress already recorded. Mirrors the `merge_captured_progress` closure inside
/// `_sync_progress_to_service`.
fn merge_captured_progress(existing: QuestProgress, captured_quests: &HashMap<String, CapturedQuest>) -> QuestProgress {
    let mut objectives = existing.objectives;

    for (quest_id, quest) in captured_quests {
        let is_complete = matches!(quest.quest_flag(), QuestFlag::Success | QuestFlag::Complete);
        for (index, mission) in quest.missions.iter().enumerate() {
            let key = format!("captured::{quest_id}::{index}::{}", mission.content_id);
            let previous = objectives.get(&key);
            let previously_submitted = previous.map(|o| o.submitted).unwrap_or(0);
            let previously_completed = previous.map(|o| o.completed).unwrap_or(false);

            objectives.insert(
                key,
                ObjectiveProgress {
                    quest_id: Some(quest_id.clone()),
                    objective_index: Some(index as i64),
                    kind: Some(CAPTURED_OBJECTIVE_KIND.to_string()),
                    item_id: Some(mission.content_id.clone()),
                    submitted: previously_submitted.max(mission.current_value.max(0) as u32),
                    completed: previously_completed || is_complete,
                },
            );
        }
    }

    QuestProgress { objectives, items: existing.items }
}

fn recent_slice<T: Clone>(items: &[T], window: usize) -> Vec<T> {
    let start = items.len().saturating_sub(window);
    items[start..].to_vec()
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

/// A deterministic, non-cryptographic digest of `data`'s canonical JSON encoding, used only to
/// detect "did this change since last time" within one process's lifetime (see
/// [`QuestPacketHandler::has_data_changed`]).
fn content_digest<T: Serialize>(data: &T) -> u64 {
    let bytes = serde_json::to_vec(data).unwrap_or_default();
    fnv1a_64(&bytes)
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_types::{MerchantInfoInput, QuestContentInfoInput, QuestLogEntryInput, RewardInfoInput};

    /// Ported from `test_packet_handler_clear_resets_change_detection_hashes`: `has_data_changed`
    /// is an implementation detail (private, like Python's leading-underscore method), so this lives
    /// as a unit test rather than in `tests/` where only the public API is visible.
    #[test]
    fn clear_resets_change_detection_hashes() {
        let mut handler = QuestPacketHandler::new();
        let payload = serde_json::json!({"merchant": "Alchemist", "flag": 1});

        assert!(handler.has_data_changed("merchant_flags", &payload));
        assert!(!handler.has_data_changed("merchant_flags", &payload));

        handler.clear();

        assert!(handler.has_data_changed("merchant_flags", &payload));
    }

    #[test]
    fn handle_merchant_list_emits_an_event_only_when_flags_change() {
        let mut handler = QuestPacketHandler::new();
        let message = MerchantListMessage {
            merchant_list: vec![MerchantInfoInput {
                merchant_id: "DesignDataMerchant:Id_Merchant_Alchemist".to_string(),
                merchant_flag: 1,
            }],
        };

        assert_eq!(handler.handle_merchant_list(&message), vec![QuestEvent::MerchantListUpdated { merchant_count: 1 }]);
        assert!(handler.handle_merchant_list(&message).is_empty());
        assert_eq!(handler.captured_state().merchant_flags.get("Alchemist"), Some(&1));
    }

    #[test]
    fn the_same_merchant_list_twice_is_no_change_however_many_merchants() {
        let mut handler = QuestPacketHandler::new();
        let message = MerchantListMessage {
            merchant_list: ["TavernMaster", "Alchemist", "Expressman", "Woodsman", "Tailor"]
                .into_iter()
                .map(|id| MerchantInfoInput { merchant_id: id.to_string(), merchant_flag: 1 })
                .collect(),
        };

        assert_eq!(handler.handle_merchant_list(&message).len(), 1);
        assert!(handler.handle_merchant_list(&message).is_empty());
    }

    #[test]
    fn handle_quest_list_always_uses_the_synthetic_last_merchant_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProgressStore::new(dir.path().to_path_buf());
        let mut handler = QuestPacketHandler::new();

        let message = QuestListMessage {
            quests: vec![QuestInfoInput {
                quest_id: "DesignDataQuest:Id_Quest_Alchemist_01".to_string(),
                quest_flag: 1,
                missions: vec![QuestContentInfoInput { content_id: "Bandage".to_string(), content_current_value: 1 }],
                ..Default::default()
            }],
            chapters: Vec::new(),
        };

        let events = handler.handle_quest_list(&message, &store).expect("handle_quest_list");
        assert_eq!(events, vec![QuestEvent::QuestListUpdated { quest_count: 1, in_progress: 1 }]);

        let state = handler.captured_state();
        let quests = state.merchant_quests.get(LAST_MERCHANT_KEY).expect("synthetic key populated");
        // The response carries no merchant id at all, so it is never derived — matching the
        // Python source's behavior (see the handler's doc comment), not a gap in this port.
        assert_eq!(quests[0].merchant_id, "");
        assert_eq!(quests[0].quest_id, "Alchemist_01");

        let (progress, _timestamp) = store.load_progress();
        assert!(progress.objectives.contains_key("captured::Alchemist_01::0::Bandage"));
    }

    #[test]
    fn handle_quest_log_uses_real_merchant_ids_and_syncs_progress() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProgressStore::new(dir.path().to_path_buf());
        let mut handler = QuestPacketHandler::new();

        let message = QuestLogMessage {
            quest_list: vec![QuestLogEntryInput {
                merchant_id: "DesignDataMerchant:Id_Merchant_Alchemist".to_string(),
                quests: vec![QuestInfoInput {
                    quest_id: "DesignDataQuest:Id_Quest_Alchemist_01".to_string(),
                    quest_flag: 1,
                    missions: vec![QuestContentInfoInput { content_id: "Bandage".to_string(), content_current_value: 1 }],
                    ..Default::default()
                }],
                chapters: Vec::new(),
            }],
        };

        let events = handler.handle_quest_log(&message, &store).expect("handle_quest_log");
        assert_eq!(events, vec![QuestEvent::QuestLogUpdated { quest_count: 1, in_progress: 1 }]);

        let state = handler.captured_state();
        let quests = state.merchant_quests.get("Alchemist").expect("real merchant key populated");
        assert_eq!(quests[0].merchant_id, "Alchemist");

        let (progress, _timestamp) = store.load_progress();
        assert!(progress.objectives.contains_key("captured::Alchemist_01::0::Bandage"));
    }

    #[test]
    fn handle_quest_complete_marks_the_tracked_quest_complete() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProgressStore::new(dir.path().to_path_buf());
        let mut handler = QuestPacketHandler::new();

        // Seed a tracked quest the way it would really arrive: via the quest log first.
        let log_message = QuestLogMessage {
            quest_list: vec![QuestLogEntryInput {
                merchant_id: "Alchemist".to_string(),
                quests: vec![QuestInfoInput { quest_id: "Alchemist_01".to_string(), quest_flag: 1, ..Default::default() }],
                chapters: Vec::new(),
            }],
        };
        handler.handle_quest_log(&log_message, &store).expect("seed quest log");

        let complete_message = QuestCompleteMessage {
            result: 0,
            given_merchant_id: "Alchemist".to_string(),
            given_quest_id: "Alchemist_01".to_string(),
            given_chapter_id: String::new(),
            rewards: vec![RewardInfoInput {
                reward_type: "Item".to_string(),
                stock_id: "GoldCoins".to_string(),
                reward_count: 30,
            }],
        };

        let events = handler.handle_quest_complete(&complete_message, &store).expect("handle_quest_complete");
        assert_eq!(
            events,
            vec![QuestEvent::QuestCompleted {
                quest_id: "Alchemist_01".to_string(),
                merchant_id: "Alchemist".to_string(),
                reward_count: 1,
            }]
        );

        let state = handler.captured_state();
        assert_eq!(state.merchant_quests["Alchemist"][0].quest_flag(), QuestFlag::Complete);
        assert_eq!(state.recent_completions.len(), 1);
    }

    #[test]
    fn handle_quest_select_and_content_value_stack_always_emit() {
        let mut handler = QuestPacketHandler::new();

        assert_eq!(
            handler.handle_quest_select(&QuestSelectMessage { result: 0 }),
            vec![QuestEvent::QuestAccepted { result: 0 }]
        );
        assert_eq!(
            handler.handle_quest_select(&QuestSelectMessage { result: 0 }),
            vec![QuestEvent::QuestAccepted { result: 0 }],
            "a select response is always reported, even repeated"
        );
        assert_eq!(
            handler.handle_quest_content_value_stack(&QuestContentValueStackMessage { result: 0 }),
            vec![QuestEvent::QuestItemsSubmitted { result: 0 }]
        );
    }

    fn log_with(quest_id: &str, flag: i32, value: i32) -> QuestLogMessage {
        QuestLogMessage {
            quest_list: vec![QuestLogEntryInput {
                merchant_id: "DesignDataMerchant:Id_Merchant_Alchemist".to_string(),
                quests: vec![QuestInfoInput {
                    quest_id: quest_id.to_string(),
                    quest_flag: flag,
                    missions: vec![QuestContentInfoInput { content_id: "Fetch_Bandages_01".into(), content_current_value: value }],
                    ..Default::default()
                }],
                chapters: Vec::new(),
            }],
        }
    }

    #[test]
    fn a_quest_list_seen_after_the_log_still_updates_progress() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProgressStore::new(dir.path().to_path_buf());
        let mut handler = QuestPacketHandler::new();
        handler.handle_quest_log(&log_with("Alchemist_01", 1, 1), &store).expect("log");

        let list = QuestListMessage {
            quests: vec![QuestInfoInput {
                quest_id: "Alchemist_01".to_string(),
                quest_flag: 2,
                missions: vec![QuestContentInfoInput { content_id: "Fetch_Bandages_01".into(), content_current_value: 3 }],
                ..Default::default()
            }],
            chapters: Vec::new(),
        };
        handler.handle_quest_list(&list, &store).expect("list");

        let progress = handler.auto_progress();
        assert_eq!(progress.quests["Alchemist_01"].quest_flag(), QuestFlag::Success);
        let tracked = &handler.tracked().quests["Alchemist_01"];
        assert_eq!(tracked.quest.merchant_id, "Alchemist", "the list keeps the merchant the log gave");
        let (saved, _) = store.load_progress();
        let objective = &saved.objectives["captured::Alchemist_01::0::Fetch_Bandages_01"];
        assert_eq!((objective.submitted, objective.completed), (3, true));
    }

    #[test]
    fn restoring_tracked_state_brings_back_quests_and_merchant_flags() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProgressStore::new(dir.path().to_path_buf());
        let mut first = QuestPacketHandler::new();
        first.handle_quest_log(&log_with("Alchemist_01", 1, 2), &store).expect("log");
        let merchants = MerchantListMessage {
            merchant_list: vec![MerchantInfoInput { merchant_id: "Alchemist".to_string(), merchant_flag: 2 }],
        };
        first.handle_merchant_list(&merchants);

        let mut second = QuestPacketHandler::new();
        second.restore_tracked(first.tracked().clone());

        assert_eq!(second.tracked(), first.tracked());
        assert_eq!(second.captured_state().merchant_flags.get("Alchemist"), Some(&2));
        assert_eq!(second.auto_progress().quests["Alchemist_01"].missions[0].current_value, 2);
    }
}
