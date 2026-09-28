use game_data::{ItemCatalog, Rarity};
use std::path::PathBuf;

const HEATER_SHIELD: &str = r#"{"HeaterShield_5001": {
    "id": "HeaterShield_5001", "name": "Heater Shield", "rarity": "Epic", "type": "Weapon",
    "slot_type": "Secondary", "hand_type": "One Handed", "weapon_type": "Shield", "is_tradable": true,
    "max_stack_size": 1, "inventory_width": 2, "inventory_height": 3, "vendor_price": 60,
    "iconPath": "icons/Weapon/HeaterShield_5001.webp", "some_field_we_ignore": [1, 2]}}"#;

#[test]
fn parses_the_fields_we_use() {
    let catalog = ItemCatalog::from_json(HEATER_SHIELD).unwrap();
    let item = catalog.get("HeaterShield_5001").unwrap();
    assert_eq!(item.name, "Heater Shield");
    assert_eq!(item.rarity, Rarity::Epic);
    assert_eq!(item.slots(), 6);
    assert_eq!(item.vendor_price, 60);
    assert!(item.tradable);
    assert_eq!(item.icon_path.as_deref(), Some("icons/Weapon/HeaterShield_5001.webp"));
}

#[test]
fn the_type_line_reads_like_the_game() {
    let catalog = ItemCatalog::from_json(HEATER_SHIELD).unwrap();
    assert_eq!(catalog.get("HeaterShield_5001").unwrap().kind_line(), "Shield · One Handed");
    let armor = ItemCatalog::from_json(r#"{"a": {"id": "a", "name": "Robe", "rarity": "Uncommon", "type": "Armor",
        "slot_type": "Chest", "armor_type": "Cloth"}}"#).unwrap();
    assert_eq!(armor.get("a").unwrap().kind_line(), "Chest · Cloth");
    let loot = ItemCatalog::from_json(r#"{"b": {"id": "b", "name": "Silver Coin", "rarity": "Common"}}"#).unwrap();
    assert_eq!(loot.get("b").unwrap().kind_line(), "");
}

#[test]
fn rarities_parse_whatever_their_case() {
    assert_eq!(Rarity::from_name("legendary"), Rarity::Legendary);
    assert_eq!(Rarity::from_name("POOR"), Rarity::Poor);
    assert_eq!(Rarity::from_name("mythic?"), Rarity::Unknown);
}

#[test]
fn the_shipped_catalog_loads() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/items.json");
    let catalog = ItemCatalog::load(&path).unwrap();
    assert!(catalog.len() > 2000, "only {} items", catalog.len());
    let axe = catalog.get("FranciscaAxe_1001").unwrap();
    assert_eq!((axe.name.as_str(), axe.rarity), ("Francisca Axe", Rarity::Poor));
}
