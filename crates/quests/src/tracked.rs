//! What the game last showed of each quest, kept across restarts: a newer message replaces what an
//! older one said, quest by quest.
//!
//! [`crate::QuestPacketHandler`] keeps DnDTools' per-message snapshots for its events. Those let an
//! older full quest log shadow a merchant's quest list seen after it (and a turn-in seen after
//! both). This keeps one entry per quest instead, so whichever message is newest wins, and it is
//! what the app saves (through [`crate::CapturedStateStore`]) to show quest status after a restart.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::packet_handler::CapturedQuest;
use crate::packet_types::QuestFlag;

/// One quest as the game last showed it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TrackedQuest {
    #[serde(flatten)]
    pub quest: CapturedQuest,
    /// The message it came from: quests of one message share the number and later messages have
    /// larger ones. 0 when the game only reported the quest being turned in.
    #[serde(default)]
    pub capture: u64,
    /// When the game last said anything about it (Unix seconds).
    #[serde(default)]
    pub seen_at: f64,
}

/// Every quest the game has shown, the newest state of each, and the merchants' flags.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TrackedState {
    #[serde(default)]
    pub quests: BTreeMap<String, TrackedQuest>,
    /// Merchant id (normalized, e.g. "TavernMaster") to its `SMERCHANT_INFO.FLAG` value.
    #[serde(default)]
    pub merchant_flags: BTreeMap<String, u32>,
    /// The last message number handed out (see [`TrackedQuest::capture`]).
    #[serde(default)]
    pub captures: u64,
    /// When anything last changed (Unix seconds); 0 before the game has shown anything.
    #[serde(default)]
    pub last_update: f64,
}

impl TrackedState {
    /// Records the quests of one message (a merchant's quest list or the quest log). A merchant's
    /// quest list carries no merchant id, so a quest keeps the one an earlier message gave it.
    pub fn record_quests(&mut self, quests: &[CapturedQuest], now: f64) {
        self.captures += 1;
        for quest in quests.iter().filter(|quest| !quest.quest_id.is_empty()) {
            let mut quest = quest.clone();
            if quest.merchant_id.is_empty() {
                if let Some(previous) = self.quests.get(&quest.quest_id) {
                    quest.merchant_id = previous.quest.merchant_id.clone();
                }
            }
            let entry = TrackedQuest { quest, capture: self.captures, seen_at: now };
            self.quests.insert(entry.quest.quest_id.clone(), entry);
        }
        self.last_update = now;
    }

    /// A quest was turned in: it is complete, whether or not the game had shown it before.
    pub fn record_completion(&mut self, merchant_id: &str, quest_id: &str, chapter_id: &str, now: f64) {
        if quest_id.is_empty() {
            return;
        }
        let entry = self.quests.entry(quest_id.to_string()).or_insert_with(|| TrackedQuest {
            quest: CapturedQuest {
                merchant_id: merchant_id.to_string(),
                quest_id: quest_id.to_string(),
                chapter_id: chapter_id.to_string(),
                ..CapturedQuest::default()
            },
            capture: 0,
            seen_at: now,
        });
        entry.quest.quest_flag = QuestFlag::Complete.to_raw();
        for mission in &mut entry.quest.missions {
            mission.completed = true;
        }
        if entry.quest.merchant_id.is_empty() {
            entry.quest.merchant_id = merchant_id.to_string();
        }
        entry.seen_at = now;
        self.last_update = now;
    }

    /// Records a merchant's flag from the merchant list. Returns whether it changed.
    pub fn record_merchant_flag(&mut self, merchant_id: &str, flag: u32, now: f64) -> bool {
        if merchant_id.is_empty() {
            return false;
        }
        let changed = self.merchant_flags.insert(merchant_id.to_string(), flag) != Some(flag);
        if changed {
            self.last_update = now;
        }
        changed
    }

    /// Every tracked quest's latest captured state, by quest id.
    pub fn captured_quests(&self) -> impl Iterator<Item = &CapturedQuest> {
        self.quests.values().map(|tracked| &tracked.quest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet_handler::CapturedMission;

    fn quest(merchant: &str, id: &str, flag: QuestFlag, value: i32) -> CapturedQuest {
        CapturedQuest {
            merchant_id: merchant.to_string(),
            quest_id: id.to_string(),
            quest_flag: flag.to_raw(),
            missions: vec![CapturedMission { content_id: "Fetch_Bandages_01".into(), current_value: value, completed: false }],
            ..CapturedQuest::default()
        }
    }

    #[test]
    fn a_newer_message_replaces_the_quest_and_keeps_its_merchant() {
        let mut state = TrackedState::default();
        state.record_quests(&[quest("Alchemist", "Alchemist_01", QuestFlag::Progress, 1)], 10.0);
        // A merchant's quest list names no merchant.
        state.record_quests(&[quest("", "Alchemist_01", QuestFlag::Success, 3)], 20.0);

        let tracked = &state.quests["Alchemist_01"];
        assert_eq!(tracked.quest.quest_flag(), QuestFlag::Success);
        assert_eq!(tracked.quest.missions[0].current_value, 3);
        assert_eq!(tracked.quest.merchant_id, "Alchemist");
        assert_eq!((tracked.capture, tracked.seen_at), (2, 20.0));
        assert_eq!(state.captures, 2);
        assert_eq!(state.last_update, 20.0);
    }

    #[test]
    fn a_turn_in_completes_known_and_unknown_quests() {
        let mut state = TrackedState::default();
        state.record_quests(&[quest("Alchemist", "Alchemist_01", QuestFlag::Success, 3)], 10.0);

        state.record_completion("Alchemist", "Alchemist_01", "", 30.0);
        state.record_completion("Woodsman", "Woodsman_02", "", 31.0);
        state.record_completion("Woodsman", "", "", 32.0);

        let known = &state.quests["Alchemist_01"];
        assert_eq!(known.quest.quest_flag(), QuestFlag::Complete);
        assert!(known.quest.missions.iter().all(|m| m.completed));
        assert_eq!(known.capture, 1, "a turn-in doesn't make a quest part of a newer message");
        let unknown = &state.quests["Woodsman_02"];
        assert_eq!((unknown.quest.quest_flag(), unknown.capture), (QuestFlag::Complete, 0));
        assert_eq!(unknown.quest.merchant_id, "Woodsman");
        assert_eq!(state.quests.len(), 2);
    }

    #[test]
    fn merchant_flags_report_changes_only() {
        let mut state = TrackedState::default();
        assert!(state.record_merchant_flag("Alchemist", 1, 5.0));
        assert!(!state.record_merchant_flag("Alchemist", 1, 6.0));
        assert!(state.record_merchant_flag("Alchemist", 2, 7.0));
        assert!(!state.record_merchant_flag("", 2, 8.0));
        assert_eq!(state.last_update, 7.0);
    }

    #[test]
    fn survives_a_save_and_load() {
        let mut state = TrackedState::default();
        state.record_quests(&[quest("Alchemist", "Alchemist_01", QuestFlag::Progress, 2)], 10.0);
        state.record_merchant_flag("Alchemist", 2, 11.0);

        let saved = serde_json::to_value(&state).expect("serialize");
        let loaded: TrackedState = serde_json::from_value(saved).expect("deserialize");

        assert_eq!(loaded, state);
        let empty: TrackedState = serde_json::from_value(serde_json::json!({})).expect("defaults");
        assert_eq!(empty, TrackedState::default());
    }
}
