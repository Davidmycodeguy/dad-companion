//! What the player owns of what quests ask for, next steps, and the item checklist.

mod common;

use common::{character, owned, shown, World, BOUGHT, LOOTED};
use quests::view::{self, QuestState, StepAction};
use quests::QuestFlag;

/// Tavern Master done and Alchemist_01 under way, so Alchemist_01's items are wanted now.
fn world_with_items() -> World {
    let mut world = World::new();
    world.tracked.record_quests(&[shown("Alchemist", "Alchemist_01", QuestFlag::Progress, &[("Fetch_Bandages_01", 1)])], 10.0);
    world.characters = vec![character(vec![
        owned(1, "Bandage_2001", 3, 4, LOOTED),
        owned(2, "Bandage_1001", 2, 5, BOUGHT),
        owned(3, "Diamond_4001", 1, 2, LOOTED),
        owned(4, "Diamond_3001", 4, 4, LOOTED),
        owned(5, "Bandage_2001", 9, state::LOCKED_SEASONAL_STASH, LOOTED),
    ])];
    world
}

#[test]
fn objectives_show_their_item_and_what_the_player_owns_of_it() {
    let world = world_with_items();
    let alchemist = view::merchant_quests(&world.inputs(), "Alchemist").expect("alchemist");

    let bandages = &alchemist.quests[0].objectives[0];
    assert_eq!(bandages.label, "Bring 2 Bandage");
    assert_eq!((bandages.submitted, bandages.done), (1, false));
    let item = bandages.item.as_ref().expect("item");
    assert_eq!((item.id.as_deref(), item.rarity), (Some("Bandage_1001"), "Unknown"), "any grade will do");
    let have = bandages.owned.as_ref().expect("owned");
    assert_eq!((have.usable, have.other), (3, 2), "the locked seasonal stash never counts");
    assert_eq!((have.places[0].stash.as_str(), have.places[0].usable), ("Stash 4", 3));

    let diamonds = &alchemist.quests[1].objectives[1];
    assert_eq!(diamonds.label, "Bring 2 Rare Diamond");
    assert_eq!(diamonds.min_rarity, Some("Rare"));
    let item = diamonds.item.as_ref().expect("item");
    assert_eq!((item.id.as_deref(), item.rarity), (Some("Diamond_4001"), "Rare"));
    assert_eq!(item.name, "Diamond (Exquisite)", "the grade's own name; the label keeps the quest's words");
    assert_eq!(diamonds.owned.as_ref().map(|o| o.usable), Some(1), "Uncommon diamonds don't count");

    let kill = &alchemist.quests[1].objectives[0];
    assert!(kill.item.is_none() && kill.owned.is_none());
    let escape = &alchemist.quests[2].objectives[2];
    assert_eq!(escape.label, "Escape from Crypts");
}

#[test]
fn rewards_name_items_gold_and_random_drops() {
    let world = World::new();
    let alchemist = view::merchant_quests(&world.inputs(), "Alchemist").expect("alchemist");
    let rewards = &alchemist.quests[0].rewards;
    assert!(rewards[0].gold && rewards[0].count == 30);
    assert_eq!((rewards[1].label.as_str(), rewards[1].count), ("Experience", 25));
    let kit = rewards[2].item.as_ref().expect("item");
    assert_eq!((rewards[2].label.as_str(), kit.rarity), ("Surgical Kit", "Common"));
    assert_eq!((rewards[3].label.as_str(), rewards[3].rarity), ("Random Uncommon Armor", Some("Uncommon")));
}

#[test]
fn next_steps_hand_in_ready_quests_first_then_bring_looted_items() {
    let mut world = world_with_items();
    world.tracked.record_quests(&[shown("Woodsman", "Woodsman_05", QuestFlag::Success, &[])], 20.0);

    let overview = view::overview(&world.inputs());
    let first = &overview.next_steps[0];
    assert_eq!((first.action, first.merchant.as_str(), first.quest_id.as_str()), (StepAction::TurnIn, "Woodsman", "Woodsman_05"));
    let bring = overview.next_steps.iter().find(|step| step.action == StepAction::Bring).expect("a bring step");
    assert_eq!((bring.merchant.as_str(), bring.quest_title.as_str()), ("Alchemist", "Marks of Malice"));
    assert_eq!((bring.items[0].count, bring.items[0].needed), (1, 1));
    let alchemist = overview.merchants.iter().find(|m| m.name == "Alchemist").expect("alchemist");
    assert_eq!(alchemist.bring, 1);
    assert_eq!(overview.characters, 1);
}

#[test]
fn the_checklist_sums_what_open_quests_still_need() {
    let world = world_with_items();
    let checklist = view::quest_items(&world.inputs());
    let names: Vec<(&str, bool)> = checklist.items.iter().map(|need| (need.item.name.as_str(), need.use_in_dungeon)).collect();
    // No Bat Wing: rotating quests the game hasn't offered need nothing.
    let expected =
        [("Bandage", false), ("Bandage", true), ("Bavin", false), ("Campfire Kit", false), ("Cracked Log", false), ("Diamond (Exquisite)", false)];
    assert_eq!(names, expected);

    let bavin = &checklist.items[2];
    let wanted: Vec<(&str, u32)> = bavin.quests.iter().map(|q| (q.quest_id.as_str(), q.remaining)).collect();
    assert_eq!(wanted, [("Cockatrice_01", 2), ("Woodsman_05", 1)], "one row per item, every quest that needs it");

    let bandages = &checklist.items[0];
    assert_eq!(bandages.quests.len(), 1);
    assert_eq!((bandages.quests[0].remaining, bandages.quests[0].state), (1, QuestState::Active));
    assert_eq!(bandages.owned.usable, 3);
    let rare = checklist.items.iter().find(|need| need.min_rarity == Some("Rare")).expect("rare diamonds");
    assert_eq!((rare.quests[0].quest_id.as_str(), rare.quests[0].state), ("Alchemist_02", QuestState::Locked));
    assert_eq!(checklist.characters, 1);
}
