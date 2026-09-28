//! The tracker the app shares between its network thread and its commands: messages in, views out,
//! the player's own ticks, and what survives a restart.

mod common;

use std::path::{Path, PathBuf};

use common::{item_catalog, stash_label};
use quests::view::{QuestState, Source};
use quests::{
    Error, MerchantInfoInput, MerchantListMessage, QuestCompleteMessage, QuestContentInfoInput, QuestInfoInput,
    QuestLogEntryInput, QuestLogMessage, QuestSelectMessage, QuestTracker,
};

/// The fixture catalog as a file, and a data folder, both removed with the returned guard.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let quests_json = dir.path().join("quests.json");
    let catalog = serde_json::to_string(&serde_json::json!({"quests": [
        {"id": "TavernMaster_01", "title": "Is it you?", "merchant": "Tavern Master", "order": 3,
         "objectives": [{"type": "Kill", "count": 1, "monster": "Skeleton Footman"}]},
        {"id": "Alchemist_01", "title": "Marks of Malice", "merchant": "Alchemist", "prerequisite": "TavernMaster_01", "order": 132,
         "objectives": [{"type": "Fetch", "count": 2, "item_id": "Bandage"}, {"type": "Kill", "count": 1, "monster": "Mummy"}]},
        {"id": "Woodsman_01", "title": "First Steps", "merchant": "Woodsman", "order": 92,
         "objectives": [{"type": "Fetch", "count": 1, "item_id": "Bavin"}]}
    ]}))
    .expect("json");
    std::fs::write(&quests_json, catalog).expect("write catalog");
    let data = dir.path().join("data").join("quests");
    (dir, quests_json, data)
}

fn open(quests_json: &Path, data: &Path) -> QuestTracker {
    QuestTracker::open(quests_json, data, &item_catalog()).expect("open tracker")
}

fn log(flag: i32, bandages: i32) -> QuestLogMessage {
    QuestLogMessage {
        quest_list: vec![QuestLogEntryInput {
            merchant_id: "DesignDataMerchant:Id_Merchant_Alchemist".into(),
            quests: vec![QuestInfoInput {
                quest_id: "QuestData:Id_Quest_Alchemist_01".into(),
                quest_flag: flag,
                missions: vec![
                    QuestContentInfoInput { content_id: "DesignDataQuestContent:Id_QuestContent_Kill_Mummy_01".into(), content_current_value: 0 },
                    QuestContentInfoInput { content_id: "DesignDataQuestContent:Id_QuestContent_Fetch_Bandages_01".into(), content_current_value: bandages },
                ],
                ..QuestInfoInput::default()
            }],
            chapters: Vec::new(),
        }],
    }
}

#[test]
fn what_the_game_showed_survives_a_restart() {
    let (_dir, quests_json, data) = setup();
    let tracker = open(&quests_json, &data);
    assert!(tracker.on_quest_log(&log(1, 1)).expect("log"));
    assert!(!tracker.on_quest_log(&log(1, 1)).expect("same log again"), "nothing changed");

    let reopened = open(&quests_json, &data);
    let alchemist = reopened.merchant_quests("Alchemist", &[], &item_catalog(), &stash_label).expect("alchemist");
    let quest = &alchemist.quests[0];
    assert_eq!((quest.state, quest.source), (QuestState::Active, Some(Source::Game)));
    assert_eq!(quest.objectives[0].submitted, 1, "Bandages land on the Bandage objective");
    assert_eq!(quest.objectives[1].submitted, 0);
    assert!(alchemist.merchant.tracked);
    assert!(reopened.overview(&[], &item_catalog(), &stash_label).last_update.is_some());
}

#[test]
fn turn_ins_and_accepts_refresh_the_page() {
    let (_dir, quests_json, data) = setup();
    let tracker = open(&quests_json, &data);
    tracker.on_quest_log(&log(2, 2)).expect("log");
    assert!(tracker.on_quest_select(&QuestSelectMessage { result: 1 }));

    let complete = QuestCompleteMessage {
        result: 1,
        given_merchant_id: "DesignDataMerchant:Id_Merchant_Alchemist".into(),
        given_quest_id: "QuestData:Id_Quest_Alchemist_01".into(),
        ..QuestCompleteMessage::default()
    };
    assert!(tracker.on_quest_complete(&complete).expect("complete"));
    let alchemist = tracker.merchant_quests("Alchemist", &[], &item_catalog(), &stash_label).expect("alchemist");
    assert_eq!(alchemist.quests[0].state, QuestState::Done);
    assert!(alchemist.merchant.current.is_none(), "nothing left at the Alchemist");
}

#[test]
fn the_merchant_list_decides_which_merchants_show() {
    let (_dir, quests_json, data) = setup();
    let tracker = open(&quests_json, &data);
    let names = |tracker: &QuestTracker| -> Vec<String> {
        tracker.overview(&[], &item_catalog(), &stash_label).merchants.into_iter().map(|m| m.name).collect()
    };
    assert_eq!(names(&tracker), ["Tavern Master", "Woodsman", "Alchemist"]);

    let list = MerchantListMessage {
        merchant_list: ["TavernMaster", "Alchemist", "Expressman"]
            .into_iter()
            .map(|id| MerchantInfoInput { merchant_id: format!("DesignDataMerchant:Id_Merchant_{id}"), merchant_flag: 0 })
            .collect(),
    };
    assert!(tracker.on_merchant_list(&list).expect("list"));
    assert!(!tracker.on_merchant_list(&list).expect("same list"));
    assert_eq!(names(&tracker), ["Tavern Master", "Alchemist"]);
    assert_eq!(tracker.service().load_active_merchants(), ["TavernMaster", "Alchemist", "Expressman"]);
}

#[test]
fn the_players_own_progress_is_kept_and_can_be_cleared() {
    let (_dir, quests_json, data) = setup();
    let tracker = open(&quests_json, &data);
    let objective = |tracker: &QuestTracker| {
        let alchemist = tracker.merchant_quests("Alchemist", &[], &item_catalog(), &stash_label).expect("alchemist");
        let first = &alchemist.quests[0].objectives[0];
        (first.submitted, first.done, alchemist.quests[0].state)
    };

    tracker.set_objective("Alchemist_01", 0, Some(1), false).expect("count");
    assert_eq!(objective(&tracker), (1, false, QuestState::Locked), "still waits for the Tavern Master");
    let (progress, _) = tracker.service().load_progress();
    assert_eq!(progress.objectives["Alchemist_01::Fetch::0::Bandage"].submitted, 1, "DnDTools' key layout");

    tracker.set_objective("Alchemist_01", 0, None, true).expect("tick");
    assert_eq!(objective(&tracker).0, 2);
    tracker.set_objective("Alchemist_01", 0, None, false).expect("untick");
    assert_eq!(objective(&tracker), (1, false, QuestState::Locked), "unticking keeps what was handed in");

    tracker.set_quest_done("Alchemist_01", true).expect("done");
    assert_eq!(objective(&tracker), (2, true, QuestState::Done));
    let tavern = tracker.merchant_quests("Tavern Master", &[], &item_catalog(), &stash_label).expect("tavern");
    assert_eq!(tavern.quests[0].source, Some(Source::Implied));

    tracker.set_quest_done("Alchemist_01", false).expect("reopen");
    assert_eq!(objective(&tracker), (0, false, QuestState::Locked));

    assert!(matches!(tracker.set_objective("Nope_01", 0, None, true), Err(Error::UnknownQuest(_))));
    assert!(matches!(tracker.set_objective("Alchemist_01", 9, None, true), Err(Error::UnknownQuest(_))));
}
