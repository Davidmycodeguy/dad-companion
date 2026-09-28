use game_data::{roll_text, stat_label};

#[test]
fn percent_stats_are_stored_times_ten() {
    assert_eq!(roll_text("ActionSpeed", 17), "+1.7%");
    assert_eq!(roll_text("ArmorPenetration", 30), "+3%");
    assert_eq!(roll_text("MoveSpeedBonus", 5), "+0.5%");
}

#[test]
fn flat_stats_show_as_numbers_with_their_sign() {
    assert_eq!(roll_text("PhysicalPower", 2), "+2");
    assert_eq!(roll_text("MoveSpeed", -20), "-20");
    assert_eq!(roll_text("MaxHealthAdd", 0), "+0");
}

#[test]
fn labels_use_the_games_wording() {
    assert_eq!(stat_label("PhysicalWeaponDamageAdd"), "Weapon Damage");
    assert_eq!(stat_label("MaxHealthBonus"), "Max Health");
    assert_eq!(stat_label("MagicRegistance"), "Magic Resistance");
}

#[test]
fn other_labels_split_their_words() {
    assert_eq!(stat_label("ArmorPenetration"), "Armor Penetration");
    assert_eq!(stat_label("Luck"), "Luck");
}
