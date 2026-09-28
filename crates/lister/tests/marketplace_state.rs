//! Port of DnDTools' `tests/test_marketplace_state.py`.

use std::sync::Arc;
use std::time::Duration;

use lister::marketplace_state::{
    describe_fail_code, ItemListInfo, ItemListMessage, MarketplaceState, MyItemInfo, MyItemListMessage, PropertyEntry, RegisterOutcome,
    RegisterResMessage, TransferResMessage,
};
use lister::FakeClock;

const SHORT: f64 = 0.05;

fn my_list(available: &[i64], unique_ids: &[u64]) -> MyItemListMessage {
    MyItemListMessage {
        available_order_indexes: available.to_vec(),
        current_page: 0,
        my_item_infos: unique_ids
            .iter()
            .map(|&uid| MyItemInfo { order_index: 0, my_item_state: 0, item_unique_id: uid, item_id: String::new(), price: 500, listing_id: 0 })
            .collect(),
    }
}

fn item_list(prices: &[i64], item_name: &str) -> ItemListMessage {
    ItemListMessage {
        current_page: 1,
        max_page: 13,
        item_infos: prices
            .iter()
            .map(|&price| ItemListInfo {
                item_id: format!("DesignDataItem:Id_Item_{item_name}"),
                price,
                item_count: 0,
                listing_id: 0,
                primary_properties: vec![],
                secondary_properties: vec![PropertyEntry {
                    property_type_id: "DesignDataItemPropertyType:Id_ItemPropertyType_Effect_Luck".to_string(),
                    property_value: 17,
                }],
            })
            .collect(),
    }
}

#[test]
fn snapshot_none_until_listing_packet() {
    assert!(MarketplaceState::default().snapshot().is_none());
}

#[test]
fn my_item_list_records_free_spots_not_total_item_count() {
    // Real packet: totalItemCount=35 while only spots 0-1 were used; free spots are the truth.
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MarketplaceState::new(clock);
    state.handle_my_item_list(&my_list(&[2, 3, 4], &[]));
    let snap = state.snapshot().unwrap();
    assert_eq!((snap.free(), snap.available.as_slice(), snap.received_at), (3, &[2, 3, 4][..], 100.0));
}

#[test]
fn register_success_and_failure() {
    let state = MarketplaceState::default();
    state.begin_register();
    state.handle_register_res(RegisterResMessage { result: 1 });
    assert_eq!(state.wait_for_register(0.1), RegisterOutcome::Ok);
    state.begin_register();
    state.handle_register_res(RegisterResMessage { result: 657 });
    assert_eq!(state.wait_for_register(0.1), RegisterOutcome::Failed(657));
}

#[test]
fn begin_register_discards_stale_result() {
    let state = MarketplaceState::default();
    state.handle_register_res(RegisterResMessage { result: 1 });
    state.begin_register();
    assert_eq!(state.wait_for_register(SHORT), RegisterOutcome::Timeout);
}

#[test]
fn wait_for_register_wakes_on_packet_from_other_thread() {
    let state = Arc::new(MarketplaceState::default());
    state.begin_register();
    let signalling = Arc::clone(&state);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        signalling.handle_register_res(RegisterResMessage { result: 1 });
    });
    assert_eq!(state.wait_for_register(2.0), RegisterOutcome::Ok);
}

#[test]
fn wait_for_listing_requires_newer_snapshot_with_item() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MarketplaceState::new(clock.clone());
    state.handle_my_item_list(&my_list(&[], &[555]));
    assert!(!state.wait_for_listing("555", 100.0, SHORT)); // not newer
    clock.set(101.0);
    state.handle_my_item_list(&my_list(&[], &[555, 777]));
    assert!(state.wait_for_listing("777", 100.5, SHORT));
    assert!(!state.wait_for_listing("999", 100.5, SHORT));
}

#[test]
#[allow(clippy::type_complexity)]
fn wait_for_item_list_returns_prices_newer_than_since() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MarketplaceState::new(clock.clone());
    state.handle_item_list(&item_list(&[300, 310], "HeaterShield_5001"));
    assert!(state.wait_for_item_list(100.0, SHORT).is_none()); // not newer
    clock.set(101.0);
    state.handle_item_list(&item_list(&[300, 333], "GemRing_6001"));
    let rows = state.wait_for_item_list(100.5, SHORT).unwrap();
    let seen: Vec<(&str, i64, &[(String, i64)])> = rows.iter().map(|r| (r.item_id.as_str(), r.price, r.rolls.as_slice())).collect();
    assert_eq!(seen, [("GemRing_6001", 300, &[("Luck".to_string(), 17)][..]), ("GemRing_6001", 333, &[("Luck".to_string(), 17)][..])]);
}

#[test]
fn describe_fail_code_messages() {
    assert!(describe_fail_code(657).to_lowercase().contains("gold"));
    let message_655 = describe_fail_code(655);
    assert!(message_655.contains("655") || message_655.to_lowercase().contains("maximum"));
    assert_eq!(describe_fail_code(12345), "Marketplace error 12345");
}

#[test]
fn snapshot_records_current_page() {
    let state = MarketplaceState::default();
    state.handle_my_item_list(&MyItemListMessage { current_page: 2, ..my_list(&[], &[]) });
    assert_eq!(state.snapshot().unwrap().current_page, 2);
}

#[test]
fn listed_ids_returns_seen_unique_ids() {
    let state = MarketplaceState::default();
    assert!(state.listed_ids().is_empty());
    state.handle_my_item_list(&my_list(&[], &[11, 12]));
    assert_eq!(state.listed_ids(), std::collections::HashSet::from(["11".to_string(), "12".to_string()]));
}

#[test]
fn snapshot_lists_sold_and_expired_payouts() {
    let mut msg = my_list(&[2, 3], &[]);
    msg.my_item_infos.push(MyItemInfo {
        order_index: 1,
        my_item_state: 3,
        item_unique_id: 0,
        item_id: "DesignDataItem:Id_Item_GreatHelm_3001".to_string(),
        price: 200,
        listing_id: 0,
    });
    msg.my_item_infos.push(MyItemInfo { order_index: 0, my_item_state: 1, item_unique_id: 0, item_id: String::new(), price: 0, listing_id: 0 });
    let state = MarketplaceState::default();
    state.handle_my_item_list(&msg);
    assert_eq!(state.snapshot().unwrap().payouts, vec![(1, 3, "GreatHelm_3001".to_string(), 200)]);
}

#[test]
fn transfer_result_wait() {
    let state = MarketplaceState::default();
    state.begin_transfer();
    assert!(state.wait_for_transfer(SHORT).is_none());
    state.handle_transfer_res(TransferResMessage { result: 1 });
    assert_eq!(state.wait_for_transfer(SHORT), Some(1));
}

#[test]
fn listed_ids_only_counts_items_still_for_sale() {
    let mut msg = my_list(&[], &[555, 777]);
    msg.my_item_infos[0].my_item_state = 1; // listing
    msg.my_item_infos[1].my_item_state = 2; // expired -> back to the stash, may be relisted
    let state = MarketplaceState::default();
    state.handle_my_item_list(&msg);
    assert_eq!(state.listed_ids(), std::collections::HashSet::from(["555".to_string()]));
}

#[test]
fn own_listing_ids_and_fresh_snapshot() {
    let clock = Arc::new(FakeClock::new(100.0));
    let state = MarketplaceState::new(clock.clone());
    let mut msg = my_list(&[], &[555]);
    msg.my_item_infos[0].listing_id = 4242;
    msg.my_item_infos[0].my_item_state = 1;
    state.handle_my_item_list(&msg);
    assert_eq!(state.own_listing_ids(), std::collections::HashSet::from(["4242".to_string()]));
    assert!(state.wait_for_fresh_snapshot(100.0, SHORT).is_none()); // not newer than `since`
    clock.set(101.0);
    state.handle_my_item_list(&msg);
    assert_eq!(state.wait_for_fresh_snapshot(100.0, SHORT).unwrap().received_at, 101.0);
}

#[test]
fn item_list_remembers_the_page_numbers() {
    let state = MarketplaceState::default();
    assert!(state.last_item_page().is_none());
    state.handle_item_list(&ItemListMessage { item_infos: vec![], current_page: 3, max_page: 12 });
    assert_eq!(state.last_item_page(), Some((3, 12)));
}
