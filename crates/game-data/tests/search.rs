use game_data::{ItemCatalog, Rarity};

fn catalog() -> ItemCatalog {
    ItemCatalog::from_json(
        r#"{
        "Id_Item_ArmingSword_1001": {"name": "Arming Sword", "rarity": "Poor", "type": "Weapon"},
        "Id_Item_ArmingSword_5001": {"name": "Arming Sword", "rarity": "Epic", "type": "Weapon"},
        "Id_Item_ArmingSword_2001": {"name": "Arming Sword", "rarity": "Common", "type": "Weapon"},
        "Id_Item_Sword_2001": {"name": "Sword", "rarity": "Common", "type": "Weapon"},
        "Id_Item_Longsword_3001": {"name": "Longsword", "rarity": "Uncommon", "type": "Weapon"},
        "Id_Item_SwordOfJustice_7001": {"name": "Sword of Justice", "rarity": "Unique", "type": "Weapon"},
        "Id_Item_Rubysword_4001": {"name": "Ruby Swordfish", "rarity": "Rare"}
    }"#,
    )
    .unwrap()
}

fn names(query: &str, limit: usize) -> Vec<String> {
    catalog().search(query, limit).into_iter().map(|hit| hit.name.to_owned()).collect()
}

#[test]
fn exact_then_prefix_then_word_start_then_anywhere() {
    assert_eq!(names("sword", 10), ["Sword", "Sword of Justice", "Arming Sword", "Ruby Swordfish", "Longsword"]);
}

#[test]
fn variants_are_grouped_under_one_name_lowest_rarity_first() {
    let catalog = catalog();
    let hits = catalog.search("arming", 10);
    assert_eq!(hits.len(), 1);
    let rarities: Vec<Rarity> = hits[0].variants.iter().map(|item| item.rarity).collect();
    assert_eq!(rarities, [Rarity::Poor, Rarity::Common, Rarity::Epic]);
}

#[test]
fn every_word_of_the_query_must_match() {
    assert_eq!(names("sw arm", 10), ["Arming Sword"]);
    assert!(names("sword axe", 10).is_empty());
}

#[test]
fn case_and_spacing_do_not_matter() {
    assert_eq!(names("  ARMING   sword ", 10), ["Arming Sword"]);
}

#[test]
fn empty_query_finds_nothing_and_limit_is_respected() {
    assert!(names("   ", 10).is_empty());
    assert_eq!(names("sword", 2), ["Sword", "Sword of Justice"]);
}
