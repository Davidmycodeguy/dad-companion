//! Shared fakes for the run tests: a simulated game that reacts to a fake mouse the way Dark and
//! Darker's stash window does, and draws its stash and bag for a fake screen. Not a test file
//! itself; included with `mod run_fakes;`.
#![allow(dead_code)] // not every test file uses every helper

mod game;

use std::time::Duration;

use game_data::ItemCatalog;
use input::Cancel;
use sorter::plan::{build_sort_plan, PlanRequest, SortPlan};
use sorter::run::{run_sort, GridGeometry, InterferenceGuard, RunConfig, RunObserver, RunReport, RunSteps, Safety, ScreenMap, Timing};
use state::stash::{BAG, STORAGE};
use state::{Character, OwnedItem};

#[allow(unused_imports)] // each test file uses a different part
pub use game::{FakeGame, FakeItem, FakeMachine, FakeSink, GameState, BACKGROUND, EMPTY};

/// Cell size of the fake screen. Small, so captures stay tiny.
pub const JUMP: f64 = 10.0;
/// Stash grid at (300, 40), bag grid at (40, 120).
pub const GEOMETRY: GridGeometry = GridGeometry { stash_origin: (300.0, 40.0), bag_origin: (40.0, 120.0), jump: JUMP };
/// Where the fake game's selector for the storage tab (stash 4) sits.
pub const TAB_POINT: (i32, i32) = (285, 50);
/// Another stash tab the game can have open instead.
pub const OTHER_TAB: u32 = 5;

/// Ring and Gem 1x1, Dagger 1x2, Shield 2x2, Potion 1x1 stacking to 5.
pub fn catalog() -> ItemCatalog {
    ItemCatalog::from_json(
        r#"{
            "Ring_1":   {"name": "Ring",   "rarity": "Uncommon",  "inventory_width": 1, "inventory_height": 1},
            "Gem_1":    {"name": "Gem",    "rarity": "Legendary", "inventory_width": 1, "inventory_height": 1},
            "Dagger_1": {"name": "Dagger", "rarity": "Common",    "inventory_width": 1, "inventory_height": 2},
            "Shield_1": {"name": "Shield", "rarity": "Epic",      "inventory_width": 2, "inventory_height": 2},
            "Potion_1": {"name": "Potion", "rarity": "Common",    "inventory_width": 1, "inventory_height": 1, "max_stack_size": 5}
        }"#,
    )
    .expect("test catalog is valid JSON")
}

pub fn owned(unique_id: u64, item_id: &str, count: u32, inventory_id: u32, slot_id: u32) -> OwnedItem {
    OwnedItem {
        unique_id,
        item_id: item_id.to_string(),
        count,
        contents: 0,
        inventory_id,
        slot_id: Some(slot_id),
        base: Vec::new(),
        rolls: Vec::new(),
        loot_state: 0,
        tradable: true,
    }
}

/// A character with these items and the stash tabs this account really lists.
pub fn character(items: Vec<OwnedItem>) -> Character {
    Character { id: "7".into(), name: "Tester".into(), class: "Fighter".into(), level: 20, items, storages: vec![4, 20, 5, 21, 30] }
}

/// A scattered storage tab: shields, daggers and rings dumped anywhere (slots count row by row in
/// a 12-wide grid), plus one ring in the bag that stays put.
pub fn scattered() -> Character {
    character(vec![
        owned(1, "Ring_1", 1, STORAGE, 30),
        owned(2, "Shield_1", 1, STORAGE, 64),
        owned(3, "Dagger_1", 1, STORAGE, 101),
        owned(4, "Gem_1", 1, STORAGE, 7),
        owned(5, "Ring_1", 1, STORAGE, 150),
        owned(6, "Dagger_1", 1, STORAGE, 17),
        owned(7, "Shield_1", 1, STORAGE, 200),
        owned(8, "Ring_1", 1, BAG, 9),
        owned(20, "Ring_1", 1, OTHER_TAB, 0),
        owned(21, "Gem_1", 1, OTHER_TAB, 1),
    ])
}

pub fn plan_for(character: &Character, stack: bool, from_bag: Vec<u64>) -> SortPlan {
    let catalog = catalog();
    let mut request = PlanRequest::new(character, &catalog, STORAGE);
    request.stack_mode = stack;
    request.transfer_from_bag = from_bag;
    build_sort_plan(&request).expect("the test stash can be planned")
}

pub fn map() -> ScreenMap {
    ScreenMap::new(GEOMETRY, STORAGE, BAG)
}

pub fn config(tab_point: Option<(i32, i32)>) -> RunConfig {
    RunConfig { map: map(), tab_point, move_delay: Duration::ZERO, timing: Timing::INSTANT }
}

/// A safety that reacts at once: no settle time after the sorter's own moves.
pub fn safety() -> Safety {
    Safety::new(InterferenceGuard::new(InterferenceGuard::tolerance_for(JUMP), Duration::ZERO))
}

/// Runs `plan` for `character`'s storage tab against `game`.
pub fn run(game: &FakeGame, character: &Character, plan: &SortPlan, config: &RunConfig, cancel: &Cancel, observer: &mut dyn RunObserver) -> RunReport {
    let steps = RunSteps::new(character, &catalog(), STORAGE, plan).expect("the plan fits the stash");
    let mut sink = game.sink();
    let machine = game.machine();
    run_sort(&mut sink, &machine, cancel, &safety(), config, &steps, observer)
}
