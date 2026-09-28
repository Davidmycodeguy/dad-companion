//! Black-box tests of the crate's public API, exercised against the real bundled
//! `assets/quests.json` snapshot rather than a hand-built fixture — the only place this port checks
//! that the whole shipped catalog actually parses.

use std::path::Path;

use game_data::ItemCatalog;
use quests::{QuestCatalog, QuestService};

fn quests_json_path() -> &'static Path {
    Path::new("../../assets/quests.json")
}

#[test]
fn loads_the_real_bundled_quest_snapshot() {
    let catalog = QuestCatalog::load(quests_json_path()).expect("assets/quests.json should parse");
    assert!(catalog.len() > 400, "expected the full DarkerDB v2 snapshot, got {} quests", catalog.len());

    let alchemist_01 = catalog.get("Alchemist_01").expect("Alchemist_01 exists in the bundled snapshot");
    assert_eq!(alchemist_01.merchant, "Alchemist");
    assert_eq!(alchemist_01.objectives[0].item_id.as_deref(), Some("Bandage"));
}

#[test]
fn quest_service_facade_round_trips_progress_and_resolves_merchant_names() {
    let catalog = QuestCatalog::load(quests_json_path()).expect("assets/quests.json should parse");
    let item_catalog = ItemCatalog::default();
    let data_dir = tempfile::tempdir().expect("temp dir");
    let service = QuestService::new(catalog, &item_catalog, data_dir.path().to_path_buf());

    assert_eq!(service.normalize_merchant_name(Some("Huntress Weekly")), "Huntress");
    assert!(service.quest_count() > 400);
    assert!(service.quest("Alchemist_01").is_some());

    let mut progress = service.default_progress_payload();
    progress.items.insert("Bandage".to_string(), 3);
    assert!(service.save_progress(&progress, Some(&["Alchemist".to_string()]), None).expect("save_progress"));

    let (loaded, _timestamp) = service.load_progress();
    assert_eq!(loaded.items["Bandage"], 3);
    assert_eq!(service.load_active_merchants(), vec!["Alchemist".to_string()]);

    let protected = service.protected_filenames();
    assert!(protected.contains("quests_progress.json"));
    assert!(protected.contains("quests_captured.json"));
}
