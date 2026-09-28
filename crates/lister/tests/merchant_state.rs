//! Port of DnDTools' `tests/test_merchant_state.py`.

use std::sync::Arc;
use std::time::Duration;

use lister::merchant_state::{MerchantState, QuestListMessage, SellBackMessage, SELL_SUCCESS};
use lister::FakeClock;

const SHORT: f64 = 0.05;

fn quests(quest_ids: &[&str]) -> QuestListMessage {
    QuestListMessage { quest_ids: quest_ids.iter().map(|s| s.to_string()).collect() }
}

fn sold(unique_ids: &[i64], result: i64) -> SellBackMessage {
    SellBackMessage { result, delete_unique_ids: unique_ids.to_vec() }
}

#[test]
fn quest_list_names_the_opened_merchant() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MerchantState::new(clock.clone());
    clock.set(101.0);
    state.handle_quest_list(&quests(&["QuestData:Id_Quest_TheCollector_01", "QuestData:Id_Quest_TheCollector_02"]));
    assert!(state.wait_for_merchant("TheCollector", 100.5, SHORT));
    assert!(!state.wait_for_merchant("Weaponsmith", 100.5, SHORT));
}

/// `requiredQuestMerchantId` names a prerequisite merchant (Alchemist_01 requires TavernMaster_01),
/// not the owner, so DnDTools' quest-list handler drops it entirely — there is no field for it on
/// [`QuestListMessage`] to even set.
#[test]
fn a_quest_that_only_requires_the_merchant_does_not_name_it() {
    let state = MerchantState::default();
    state.handle_quest_list(&quests(&["QuestData:Id_Quest_Alchemist_01"]));
    assert!(!state.wait_for_merchant("TheCollector", 99.0, SHORT));
}

#[test]
fn merchant_key_must_match_whole_name() {
    let state = MerchantState::default();
    state.handle_quest_list(&quests(&["QuestData:Id_Quest_TheCollectorX_01"]));
    assert!(!state.wait_for_merchant("TheCollector", 99.0, SHORT));
}

#[test]
fn old_quest_lists_do_not_count() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MerchantState::new(clock);
    state.handle_quest_list(&quests(&["QuestData:Id_Quest_TheCollector_01"]));
    assert!(!state.wait_for_merchant("TheCollector", 100.0, SHORT));
}

#[test]
fn wait_for_merchant_wakes_when_the_list_arrives() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = Arc::new(MerchantState::new(clock.clone()));
    let signalling = Arc::clone(&state);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        clock.set(102.0);
        signalling.handle_quest_list(&quests(&["QuestData:Id_Quest_TheCollector_01"]));
    });
    assert!(state.wait_for_merchant("TheCollector", 101.0, 2.0));
}

#[test]
fn sell_reply_reports_result_and_sold_ids() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MerchantState::new(clock.clone());
    clock.set(105.0);
    state.handle_sell_back(&sold(&[722_759_077_045_754_889, 5], SELL_SUCCESS));
    let reply = state.wait_for_sell_back(104.0, SHORT).unwrap();
    assert_eq!(reply.result, SELL_SUCCESS);
    assert_eq!(reply.deleted_ids, vec!["722759077045754889".to_string(), "5".to_string()]);
}

#[test]
fn sell_reply_before_since_is_ignored() {
    let state = MerchantState::default();
    state.handle_sell_back(&sold(&[1], SELL_SUCCESS));
    assert!(state.wait_for_sell_back(100.0, SHORT).is_none());
}

#[test]
fn failed_sell_reply_is_reported() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MerchantState::new(clock.clone());
    clock.set(101.0);
    state.handle_sell_back(&sold(&[], 7));
    let reply = state.wait_for_sell_back(100.0, SHORT).unwrap();
    assert_eq!(reply.result, 7);
    assert!(reply.deleted_ids.is_empty());
}

/// `deleteUniqueIds` is a signed `int64` while `itemUniqueId` is `uint64`: ids from `2**63` arrive
/// negative and must be read back as the unsigned id the game actually means.
#[test]
fn signed_ids_are_read_as_the_unsigned_item_ids() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MerchantState::new(clock.clone());
    clock.set(101.0);
    state.handle_sell_back(&sold(&[-1], SELL_SUCCESS));
    let reply = state.wait_for_sell_back(100.0, SHORT).unwrap();
    assert_eq!(reply.deleted_ids, vec![(u64::MAX).to_string()]);
}
