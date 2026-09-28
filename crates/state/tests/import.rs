use serde_json::json;
use state::import::character_from_saved;

#[test]
fn a_saved_character_becomes_a_character_with_all_its_stashes() {
    let saved = json!({
        "result": 1,
        "characterDataBase": {
            "characterId": "10000001",
            "characterClass": "DesignDataPlayerCharacter:Id_PlayerCharacter_Fighter",
            "level": 66,
            "nickName": {"originalNickName": "Adventurer"},
            "CharacterItemList": [{
                "itemUniqueId": "724684334287562769",
                "itemId": "DesignDataItem:Id_Item_BarbutaHelm_3001",
                "itemCount": 1, "inventoryId": 3, "slotId": 1,
                "primaryPropertyArray": [{"propertyTypeId": "DesignDataItemPropertyType:Id_ItemPropertyType_Effect_ArmorRating", "propertyValue": 30}],
                "secondaryPropertyArray": [{"propertyTypeId": "DesignDataItemPropertyType:Id_ItemPropertyType_Effect_Luck", "propertyValue": 17}],
                "tradable": 1
            }],
            "CharacterStorageInfos": [{
                "inventoryId": 4,
                "CharacterStorageItemList": [
                    {"itemUniqueId": "725329825425807376", "itemId": "DesignDataItem:Id_Item_GoldCoins", "itemCount": 25, "slotId": 221, "lootState": 3, "tradable": 1},
                    {"itemUniqueId": "1", "itemId": "DesignDataItem:Id_Item_Bandage_2001"},
                    {"itemUniqueId": "2", "itemId": "DesignDataItem:Id_Item_GoldCoinBag", "itemContentsCount": 2500, "slotId": 16}
                ]
            }]
        }
    });

    let character = character_from_saved(&saved).unwrap();

    assert_eq!((character.id.as_str(), character.name.as_str(), character.class.as_str(), character.level), ("10000001", "Adventurer", "Fighter", 66));
    assert_eq!(character.items.len(), 4);
    assert_eq!(character.items[3].contents, 2500); // gold inside the bag
    let helm = &character.items[0];
    assert_eq!((helm.item_id.as_str(), helm.inventory_id, helm.slot_id, helm.unique_id), ("BarbutaHelm_3001", 3, Some(1), 724_684_334_287_562_769));
    assert_eq!(helm.base, [("ArmorRating".to_owned(), 30)]);
    assert_eq!(helm.rolls, [("Luck".to_owned(), 17)]);
    assert!(helm.tradable);
    let coins = &character.items[1];
    assert_eq!((coins.item_id.as_str(), coins.count, coins.inventory_id, coins.slot_id, coins.loot_state), ("GoldCoins", 25, 4, Some(221), 3));
    let bandage = &character.items[2];
    assert_eq!((bandage.inventory_id, bandage.slot_id, bandage.count, bandage.tradable), (4, None, 1, false)); // storage's id; defaults
}

#[test]
fn anything_else_is_not_a_character() {
    assert!(character_from_saved(&json!({"version": 1})).is_err());
}
