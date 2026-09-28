//! The Quests page: merchants' quest chains, what each quest still needs, and the items to keep
//! for them, updated live while the player talks to merchants in game. Thin on purpose: the views
//! and the bookkeeping live in the `quests` crate (tested there); this converts the game's decoded
//! merchant messages for it and exposes the page's commands.

use std::path::Path;

use tauri::State;

use protocol::messages::{proto, Decoded};
use quests::view::{MerchantQuests, QuestItems, QuestsOverview};
use quests::{
    MerchantInfoInput, MerchantListMessage, QuestChapterInfoInput, QuestCompleteMessage, QuestContentInfoInput,
    QuestContentValueStackMessage, QuestInfoInput, QuestListMessage, QuestLogEntryInput, QuestLogMessage,
    QuestSelectMessage, QuestTracker, RewardInfoInput,
};

use crate::state::AppState;

/// The quest catalog inside the assets folder.
const QUESTS_FILE: &str = "quests.json";
/// Where quest progress is kept inside a data folder (DnDTools used the same name).
const QUESTS_FOLDER: &str = "quests";

/// Quest tracking: the catalog, the player's progress, and what the game last showed. Shared by
/// the network thread (`handle`) and the commands; the tracker locks what it needs.
pub struct Quests {
    tracker: QuestTracker,
}

impl Quests {
    /// Opens the catalog in `assets` and the progress kept under `data_dir/quests`, first taking
    /// over the progress DnDTools saved when there is none yet (a copy; DnDTools' file is untouched).
    pub fn load(assets: &Path, data_dir: &Path, catalog: &game_data::ItemCatalog) -> Result<Quests, String> {
        let folder = data_dir.join(QUESTS_FOLDER);
        if let Some(dndtools) = appdata::DataDir::dndtools_root() {
            match quests::import::import_progress_once(&dndtools.join("data").join(QUESTS_FOLDER), &folder) {
                Ok(true) => log::info!("quest progress taken over from DnDTools"),
                Ok(false) => {}
                Err(err) => log::warn!("DnDTools' quest progress could not be taken over: {err}"),
            }
        }
        let quests_json = assets.join(QUESTS_FILE);
        let tracker = QuestTracker::open(&quests_json, &folder, catalog)
            .map_err(|err| format!("quest data ({}) could not be read: {err}", quests_json.display()))?;
        Ok(Self { tracker })
    }

    /// Takes in one decoded game message. Returns true when the Quests page should refresh; every
    /// message that isn't about merchants' quests is ignored.
    pub fn handle(&self, decoded: &Decoded) -> bool {
        let result = match decoded {
            Decoded::MerchantList(list) => self.tracker.on_merchant_list(&merchant_list(list)),
            Decoded::MerchantQuestList(list) => self.tracker.on_quest_list(&quest_list(list)),
            Decoded::MerchantQuestLogList(log) => self.tracker.on_quest_log(&quest_log(log)),
            Decoded::MerchantQuestComplete(done) => self.tracker.on_quest_complete(&quest_complete(done)),
            Decoded::MerchantQuestSelect(select) => {
                Ok(self.tracker.on_quest_select(&QuestSelectMessage { result: result_code(select.result) }))
            }
            Decoded::MerchantQuestContentValueStack(stack) => Ok(self
                .tracker
                .on_content_value_stack(&QuestContentValueStackMessage { result: result_code(stack.result) })),
            _ => return false,
        };
        result.unwrap_or_else(|err| {
            // What the game showed is still applied in memory; only saving it failed.
            log::warn!("quest progress could not be saved: {err}");
            true
        })
    }
}

fn result_code(result: u32) -> i32 {
    i32::try_from(result).unwrap_or(i32::MAX)
}

fn merchant_list(list: &proto::Ss2cMerchantListRes) -> MerchantListMessage {
    MerchantListMessage {
        merchant_list: list
            .merchant_list
            .iter()
            .map(|merchant| MerchantInfoInput { merchant_id: merchant.merchant_id.clone(), merchant_flag: merchant.merchant_flag })
            .collect(),
    }
}

fn quest_info(info: &proto::SmerchantQuestInfo) -> QuestInfoInput {
    QuestInfoInput {
        quest_order: info.quest_order,
        quest_id: info.quest_id.clone(),
        chapter_id: info.chapter_id.clone(),
        quest_flag: info.quest_flag,
        already_get_affinity: info.already_get_affinity,
        missions: info
            .missions
            .iter()
            .map(|mission| QuestContentInfoInput {
                content_id: mission.content_id.clone(),
                content_current_value: mission.content_current_value,
            })
            .collect(),
    }
}

fn chapter(info: &proto::SmerchantQuestChapterInfo) -> QuestChapterInfoInput {
    QuestChapterInfoInput { chapter_id: info.chapter_id.clone(), remain_ms_time: info.remain_ms_time }
}

fn quest_list(list: &proto::Ss2cMerchantQuestListInfoRes) -> QuestListMessage {
    QuestListMessage { quests: list.quests.iter().map(quest_info).collect(), chapters: list.chapters.iter().map(chapter).collect() }
}

fn quest_log(log: &proto::Ss2cMerchantQuestLogListRes) -> QuestLogMessage {
    QuestLogMessage {
        quest_list: log
            .quest_list
            .iter()
            .map(|entry| QuestLogEntryInput {
                merchant_id: entry.merchant_id.clone(),
                quests: entry.quests.iter().map(quest_info).collect(),
                chapters: entry.chapters.iter().map(chapter).collect(),
            })
            .collect(),
    }
}

fn quest_complete(done: &proto::Ss2cMerchantQuestCompleteRes) -> QuestCompleteMessage {
    QuestCompleteMessage {
        result: result_code(done.result),
        given_merchant_id: done.given_merchant_id.clone(),
        given_quest_id: done.given_quest_id.clone(),
        given_chapter_id: done.given_chapter_id.clone(),
        rewards: done
            .rewards
            .iter()
            .map(|reward| RewardInfoInput {
                reward_type: reward.reward_type.clone(),
                stock_id: reward.stock_id.clone(),
                reward_count: reward.reward_count,
            })
            .collect(),
    }
}

fn quest_tracker(state: &AppState) -> Result<&QuestTracker, String> {
    state.quests.as_ref().map(|quests| &quests.tracker).ok_or_else(|| "Quest data could not be loaded.".to_string())
}

/// The player's characters, for counting what they own of what quests ask for.
fn characters(state: &AppState) -> Vec<state::Character> {
    state.characters.lock().map(|characters| characters.clone()).unwrap_or_default()
}

/// Every merchant with quests to show, and what the player can do at each now.
#[tauri::command]
pub fn quests_overview(state: State<'_, AppState>) -> Result<QuestsOverview, String> {
    Ok(quest_tracker(&state)?.overview(&characters(&state), &state.catalog, &crate::stash_view::stash_label))
}

/// One merchant's quests in chain order.
#[tauri::command]
pub fn quests_merchant(state: State<'_, AppState>, merchant: String) -> Result<MerchantQuests, String> {
    quest_tracker(&state)?
        .merchant_quests(&merchant, &characters(&state), &state.catalog, &crate::stash_view::stash_label)
        .ok_or_else(|| format!("{merchant} has no quests to show."))
}

/// Every item open quests still need, with what the player owns of it.
#[tauri::command]
pub fn quests_items(state: State<'_, AppState>) -> Result<QuestItems, String> {
    Ok(quest_tracker(&state)?.quest_items(&characters(&state), &state.catalog, &crate::stash_view::stash_label))
}

/// The player's own progress on one objective (`index` counts from 0): how many are handed in
/// (None: all when done, else what was recorded) and whether it's done.
#[tauri::command]
pub fn quests_set_objective(
    state: State<'_, AppState>,
    quest_id: String,
    index: usize,
    submitted: Option<u32>,
    done: bool,
) -> Result<(), String> {
    quest_tracker(&state)?.set_objective(&quest_id, index, submitted, done).map_err(|err| err.to_string())
}

/// Ticks every objective of a quest done, or clears the player's own progress on it.
#[tauri::command]
pub fn quests_set_done(state: State<'_, AppState>, quest_id: String, done: bool) -> Result<(), String> {
    quest_tracker(&state)?.set_quest_done(&quest_id, done).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quest_info(quest_id: &str, flag: i32, content_id: &str, value: i32) -> proto::SmerchantQuestInfo {
        proto::SmerchantQuestInfo {
            quest_order: 3,
            quest_id: quest_id.to_string(),
            chapter_id: "DesignDataQuestChapter:Id_QuestChapter_Alchemist_01".to_string(),
            quest_flag: flag,
            already_get_affinity: 1,
            missions: vec![proto::SmerchantQuestContentInfo { content_id: content_id.to_string(), content_current_value: value }],
            required_quest_merchant_id: String::new(),
        }
    }

    fn log_with(value: i32) -> proto::Ss2cMerchantQuestLogListRes {
        proto::Ss2cMerchantQuestLogListRes {
            result: 1,
            quest_list: vec![proto::SmerchantQuestLogInfo {
                merchant_id: "DesignDataMerchant:Id_Merchant_Alchemist".to_string(),
                quests: vec![quest_info("QuestData:Id_Quest_Alchemist_01", 1, "Fetch_Bandages_01", value)],
                chapters: vec![proto::SmerchantQuestChapterInfo { chapter_id: "c".to_string(), remain_ms_time: 5 }],
            }],
        }
    }

    #[test]
    fn quest_messages_convert_every_field_the_tracker_reads() {
        let log = log_with(2);
        let converted = quest_log(&log);
        let entry = &converted.quest_list[0];
        assert_eq!(entry.merchant_id, "DesignDataMerchant:Id_Merchant_Alchemist");
        let quest = &entry.quests[0];
        assert_eq!((quest.quest_order, quest.quest_flag, quest.already_get_affinity), (3, 1, 1));
        assert_eq!((quest.missions[0].content_id.as_str(), quest.missions[0].content_current_value), ("Fetch_Bandages_01", 2));
        assert_eq!((entry.chapters[0].chapter_id.as_str(), entry.chapters[0].remain_ms_time), ("c", 5));

        let list = proto::Ss2cMerchantQuestListInfoRes { result: 1, quests: log.quest_list[0].quests.clone(), chapters: Vec::new() };
        assert_eq!(quest_list(&list).quests, entry.quests);

        let done = proto::Ss2cMerchantQuestCompleteRes {
            result: 1,
            given_merchant_id: "m".to_string(),
            given_quest_id: "q".to_string(),
            given_chapter_id: "c".to_string(),
            rewards: vec![proto::SrewardInfo { reward_type: "Item".to_string(), stock_id: "GoldCoins".to_string(), reward_count: 30 }],
        };
        let complete = quest_complete(&done);
        assert_eq!((complete.result, complete.given_quest_id.as_str()), (1, "q"));
        assert_eq!((complete.rewards[0].stock_id.as_str(), complete.rewards[0].reward_count), ("GoldCoins", 30));

        let merchants = proto::Ss2cMerchantListRes {
            merchant_list: vec![proto::SmerchantInfo { merchant_id: "Alchemist".to_string(), merchant_flag: 2, ..Default::default() }],
        };
        assert_eq!(merchant_list(&merchants).merchant_list[0].merchant_flag, 2);
        assert_eq!(result_code(u32::MAX), i32::MAX);
    }

    #[test]
    fn handle_refreshes_on_quest_messages_and_ignores_the_rest() {
        let folder = std::env::temp_dir().join(format!("dad-companion-quests-view-{}", std::process::id()));
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let tracker =
            QuestTracker::open(&assets.join(QUESTS_FILE), &folder, &game_data::ItemCatalog::default()).expect("open tracker");
        let quests = Quests { tracker };

        assert!(quests.handle(&Decoded::MerchantQuestLogList(log_with(1))));
        assert!(quests.handle(&Decoded::MerchantQuestSelect(proto::Ss2cMerchantQuestSelectRes { result: 1 })));
        assert!(!quests.handle(&Decoded::MerchantStock(proto::Ss2cMerchantStockBuyItemListRes::default())));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
