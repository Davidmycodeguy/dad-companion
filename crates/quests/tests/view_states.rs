//! Where quests stand: the game's word, the player's ticks, chains, rotations and merchants.

mod common;

use common::{shown, World};
use quests::view::{self, QuestState, QuestView, Source};
use quests::{ObjectiveProgress, QuestFlag};

fn quest<'a>(quests: &'a [QuestView], id: &str) -> &'a QuestView {
    quests.iter().find(|quest| quest.id == id).unwrap_or_else(|| panic!("{id} is shown"))
}

#[test]
fn nothing_known_yet_means_chains_start_open_and_the_rest_waits() {
    let world = World::new();
    let alchemist = view::merchant_quests(&world.inputs(), "Alchemist").expect("alchemist");

    let ids: Vec<&str> = alchemist.quests.iter().map(|quest| quest.id.as_str()).collect();
    assert_eq!(ids, ["Alchemist_01", "Alchemist_02", "Alchemist_03"]);
    assert!(alchemist.quests.iter().all(|quest| quest.state == QuestState::Locked), "Tavern Master comes first");
    let first = quest(&alchemist.quests, "Alchemist_01");
    let before = first.prerequisite.as_ref().expect("prerequisite");
    assert_eq!((before.title.as_str(), before.merchant.as_str(), before.done), ("Is it you?", "Tavern Master", false));

    let tavern = view::merchant_quests(&world.inputs(), "tavern master").expect("any case");
    assert_eq!(tavern.quests[0].state, QuestState::Available);
    assert_eq!(tavern.merchant.current.as_ref().map(|c| c.id.as_str()), Some("TavernMaster_01"));
}

#[test]
fn a_quest_the_game_shows_open_means_its_chain_before_it_is_done() {
    let mut world = World::new();
    let log = [shown("Alchemist", "Alchemist_02", QuestFlag::Progress, &[("Kill_LivingArmor_01", 1)])];
    world.tracked.record_quests(&log, 100.0);

    let alchemist = view::merchant_quests(&world.inputs(), "Alchemist").expect("alchemist");
    let first = quest(&alchemist.quests, "Alchemist_01");
    assert_eq!((first.state, first.source), (QuestState::Done, Some(Source::Implied)));
    assert!(first.objectives.iter().all(|objective| objective.done && objective.submitted == objective.count));

    let second = quest(&alchemist.quests, "Alchemist_02");
    assert_eq!((second.state, second.source, second.seen_at), (QuestState::Active, Some(Source::Game), Some(100.0)));
    assert_eq!((second.objectives[0].submitted, second.objectives[0].source), (1, Some(Source::Game)));
    assert_eq!(second.objectives[1].submitted, 0);
    assert_eq!(quest(&alchemist.quests, "Alchemist_03").state, QuestState::Locked);

    let tavern = view::merchant_quests(&world.inputs(), "Tavern Master").expect("tavern");
    assert_eq!(tavern.quests[0].state, QuestState::Done, "cross-merchant prerequisites are implied too");
    assert!(alchemist.merchant.tracked);
    assert!(!tavern.merchant.tracked);
}

#[test]
fn ready_and_handed_in_quests_come_from_the_game() {
    let mut world = World::new();
    world.tracked.record_quests(&[shown("Alchemist", "Alchemist_01", QuestFlag::Success, &[("Fetch_Bandages_01", 2)])], 10.0);
    world.tracked.record_completion("TavernMaster", "TavernMaster_01", "", 11.0);

    let alchemist = view::merchant_quests(&world.inputs(), "Alchemist").expect("alchemist");
    let ready = quest(&alchemist.quests, "Alchemist_01");
    assert_eq!(ready.state, QuestState::Ready);
    assert!(ready.objectives[0].done);
    assert_eq!(quest(&alchemist.quests, "Alchemist_02").state, QuestState::Locked, "waits until handed in");
    assert!(alchemist.merchant.turn_in);
    assert_eq!(alchemist.merchant.ready, 1);
}

#[test]
fn missions_land_on_the_objective_they_name_and_the_rest_is_a_tally() {
    let mut world = World::new();
    let woodsman = shown("Woodsman", "Woodsman_05", QuestFlag::Progress, &[("Fetch_Bavin_02", 0), ("Fetch_CampfireKit_02", 0), ("Fetch_CrackedLog_01", 1)]);
    let cockatrice = shown("Cockatrice", "Cockatrice_01", QuestFlag::Progress, &[("Fetch_Cockatrice_01_02", 2), ("Fetch_Cockatrice_01_01", 1)]);
    world.tracked.record_quests(&[woodsman, cockatrice], 50.0);

    let woods = view::merchant_quests(&world.inputs(), "Woodsman").expect("woodsman");
    let done: Vec<bool> = woods.quests[0].objectives.iter().map(|objective| objective.done).collect();
    assert_eq!(done, [false, true, false], "Cracked Log is the catalog's second objective");

    let cock = view::merchant_quests(&world.inputs(), "Cockatrice").expect("cockatrice");
    let tally = cock.quests[0].unmatched.expect("a tally");
    assert_eq!((tally.submitted, tally.needed), (3, 4));
    assert!(cock.quests[0].objectives.iter().all(|objective| objective.submitted == 0 && objective.source.is_none()));
}

#[test]
fn the_players_own_ticks_count_and_finish_quests() {
    let mut world = World::new();
    let tick = |quest_id: &str, index: i64, submitted: u32, completed: bool| ObjectiveProgress {
        quest_id: Some(quest_id.to_string()),
        objective_index: Some(index),
        kind: Some("Fetch".into()),
        item_id: None,
        submitted,
        completed,
    };
    // DnDTools' key layout, and one entry with only the key to go by.
    world.progress.objectives.insert("Alchemist_02::Kill::0::Living Armor".into(), tick("Alchemist_02", 0, 2, true));
    world.progress.objectives.insert("Alchemist_02::Fetch::1::Diamond".into(), tick("Alchemist_02", 1, 2, true));
    let partial = ObjectiveProgress { submitted: 1, ..ObjectiveProgress::default() };
    world.progress.objectives.insert("Alchemist_03::Use Item::1::Bandage".into(), partial);

    let alchemist = view::merchant_quests(&world.inputs(), "Alchemist").expect("alchemist");
    let second = quest(&alchemist.quests, "Alchemist_02");
    assert_eq!((second.state, second.source), (QuestState::Done, Some(Source::Manual)));
    assert_eq!(quest(&alchemist.quests, "Alchemist_01").source, Some(Source::Implied));
    let third = quest(&alchemist.quests, "Alchemist_03");
    assert_eq!((third.state, third.objectives[1].submitted), (QuestState::Active, 1));
    assert_eq!(third.objectives[1].source, Some(Source::Manual));
}

#[test]
fn rotating_quests_show_only_while_the_game_offers_them() {
    let mut world = World::new();
    assert!(view::merchant_quests(&world.inputs(), "Huntress").is_none(), "nothing offered yet");
    let names = |world: &World| -> Vec<String> { view::overview(&world.inputs()).merchants.into_iter().map(|m| m.name).collect() };
    assert!(!names(&world).contains(&"Huntress".to_string()));

    world.tracked.record_quests(
        &[
            shown("Huntress", "Huntress_Daily_01", QuestFlag::Progress, &[("Kill_Huntress_GoblinMage_01", 0)]),
            shown("Huntress", "Huntress_Daily_02", QuestFlag::Available, &[]),
        ],
        10.0,
    );
    // The next day's offer: a weekly, while the first daily is still under way.
    world.tracked.record_quests(&[shown("", "Huntress_Weekly_01", QuestFlag::Available, &[])], 20.0);

    let huntress = view::merchant_quests(&world.inputs(), "Huntress").expect("offered now");
    let ids: Vec<&str> = huntress.quests.iter().map(|quest| quest.id.as_str()).collect();
    assert_eq!(ids, ["Huntress_Daily_01", "Huntress_Weekly_01"]);
    assert_eq!(huntress.quests[0].time_limit, Some("Daily"));
    assert_eq!(huntress.quests[1].time_limit, Some("Weekly"));
    assert!(names(&world).contains(&"Huntress".to_string()));
}

#[test]
fn once_the_game_lists_its_merchants_only_those_show() {
    let mut world = World::new();
    let all = view::overview(&world.inputs());
    let names: Vec<&str> = all.merchants.iter().map(|m| m.name.as_str()).collect();
    // No Valentine: its only quest is a placeholder with no title or text yet.
    assert_eq!(names, ["Tavern Master", "Cockatrice", "Woodsman", "Alchemist"], "in the order the game introduces them");
    assert!(view::merchant_quests(&world.inputs(), "Valentine").is_none());
    assert_eq!(all.last_update, None);

    world.active = vec!["TavernMaster".into(), "Alchemist".into(), "Expressman".into()];
    world.tracked.record_quests(&[shown("Woodsman", "Woodsman_05", QuestFlag::Progress, &[])], 30.0);
    let listed = view::overview(&world.inputs());
    let names: Vec<&str> = listed.merchants.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Tavern Master", "Woodsman", "Alchemist"], "plus merchants the game showed quests for");
    assert_eq!(listed.last_update, Some(30.0));
}

#[test]
fn merchant_cards_count_quests_and_name_the_next_one() {
    let mut world = World::new();
    world.tracked.record_quests(&[shown("Alchemist", "Alchemist_02", QuestFlag::Progress, &[])], 5.0);
    world.tracked.record_merchant_flag("Alchemist", 2, 6.0);

    let overview = view::overview(&world.inputs());
    let alchemist = overview.merchants.iter().find(|m| m.name == "Alchemist").expect("alchemist");
    assert_eq!((alchemist.total, alchemist.done, alchemist.active, alchemist.locked), (3, 1, 1, 1));
    assert!(alchemist.turn_in, "the game flags a quest ready to hand in");
    let current = alchemist.current.as_ref().expect("current");
    assert_eq!((current.id.as_str(), current.state), ("Alchemist_02", QuestState::Active));
    assert_eq!(alchemist.seen_at, Some(5.0));

    let cockatrice = overview.merchants.iter().find(|m| m.name == "Cockatrice").expect("cockatrice");
    assert!(!cockatrice.turn_in && !cockatrice.tracked);
}

#[test]
fn missions_saved_by_dndtools_count_until_the_game_says_more() {
    let mut world = World::new();
    let saved = |quest: &str, index: i64, content: &str, submitted: u32, completed: bool| {
        (
            format!("captured::{quest}::{index}::{content}"),
            ObjectiveProgress {
                quest_id: Some(quest.to_string()),
                objective_index: Some(index),
                kind: Some("captured".into()),
                item_id: Some(content.to_string()),
                submitted,
                completed,
            },
        )
    };
    world.progress.objectives.extend([
        saved("Woodsman_05", 0, "Fetch_CrackedLog_01", 1, true),
        saved("Woodsman_05", 1, "Fetch_Bavin_02", 1, true),
        saved("Woodsman_05", 2, "Fetch_CampfireKit_02", 1, true),
        saved("Cockatrice_01", 0, "Fetch_Cockatrice_01_03", 2, false),
        saved("Cockatrice_01", 1, "Fetch_Cockatrice_01_01", 0, false),
    ]);

    let woodsman = view::merchant_quests(&world.inputs(), "Woodsman").expect("woodsman");
    assert_eq!((woodsman.quests[0].state, woodsman.quests[0].source), (QuestState::Done, Some(Source::Game)));
    let cockatrice = view::merchant_quests(&world.inputs(), "Cockatrice").expect("cockatrice");
    assert_eq!(cockatrice.quests[0].state, QuestState::Active, "the game counted 2 handed in");
    assert_eq!(cockatrice.quests[0].unmatched.map(|t| (t.submitted, t.needed)), Some((2, 4)));
    assert!(cockatrice.merchant.tracked);

    // What the game shows now wins over what was saved.
    world.tracked.record_quests(&[shown("Woodsman", "Woodsman_05", QuestFlag::Progress, &[("Fetch_Bavin_02", 1)])], 9.0);
    let woodsman = view::merchant_quests(&world.inputs(), "Woodsman").expect("woodsman");
    assert_eq!(woodsman.quests[0].state, QuestState::Active);
}
