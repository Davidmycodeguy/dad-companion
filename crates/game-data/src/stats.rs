//! Item stats as the game shows them: "Weapon Damage +1", "Action Speed +1.7%".

/// Stats stored ×10 and shown with a % in game.
const PERCENT_STATS: [&str; 23] = [
    "ActionSpeed",
    "ArmorPenetration",
    "MagicPenetration",
    "BuffDurationBonus",
    "DebuffDurationBonus",
    "CooldownReductionBonus",
    "DemonDamageMod",
    "DemonReductionMod",
    "UndeadDamageMod",
    "UndeadReductionMod",
    "HeadshotDamageMod",
    "HeadshotReductionMod",
    "ProjectileReductionMod",
    "MagicalDamageBonus",
    "PhysicalDamageBonus",
    "MagicalDamageReduction",
    "PhysicalDamageReduction",
    "MagicalInteractionSpeed",
    "RegularInteractionSpeed",
    "SpellCastingSpeed",
    "MaxHealthBonus",
    "MoveSpeedBonus",
    "MemoryCapacityBonus",
];

/// Stats whose in-game name is not their id split into words.
const LABELS: [(&str, &str); 25] = [
    ("PhysicalWeaponDamageAdd", "Weapon Damage"),
    ("PhysicalDamageAdd", "Physical Damage"),
    ("PhysicalDamageTrue", "True Physical Damage"),
    ("MagicalDamageAdd", "Magical Damage"),
    ("MagicalDamageTrue", "True Magical Damage"),
    ("MagicRegistance", "Magic Resistance"),
    ("ArmorRatingAdd", "Armor Rating"),
    ("MaxHealthAdd", "Max Health"),
    ("MaxHealthBonus", "Max Health"),
    ("MoveSpeedAdd", "Move Speed"),
    ("MoveSpeedBonus", "Move Speed"),
    ("MemoryCapacityAdd", "Memory Capacity"),
    ("MemoryCapacityBonus", "Memory Capacity"),
    ("UndeadDamageMod", "Undead Damage"),
    ("UndeadReductionMod", "Undead Reduction"),
    ("DemonDamageMod", "Demon Damage"),
    ("DemonReductionMod", "Demon Reduction"),
    ("HeadshotDamageMod", "Headshot Damage"),
    ("HeadshotReductionMod", "Headshot Reduction"),
    ("ProjectileReductionMod", "Projectile Reduction"),
    ("CooldownReductionBonus", "Cooldown Reduction"),
    ("MagicalInteractionSpeed", "Magic Interaction"),
    ("RegularInteractionSpeed", "Interaction Speed"),
    ("PhysicalDamageBonus", "Physical Damage Bonus"),
    ("MagicalDamageBonus", "Magical Damage Bonus"),
];

/// Whether the stat is stored ×10 and shown as a percentage.
pub fn is_percent_stat(stat: &str) -> bool {
    PERCENT_STATS.contains(&stat)
}

/// The stat's name as the game writes it ("ArmorPenetration" -> "Armor Penetration").
pub fn stat_label(stat: &str) -> String {
    if let Some((_, label)) = LABELS.iter().find(|(id, _)| *id == stat) {
        return (*label).to_owned();
    }
    let mut label = String::with_capacity(stat.len() + 4);
    let mut previous_lower = false;
    for c in stat.chars() {
        if c.is_ascii_uppercase() && previous_lower {
            label.push(' ');
        }
        previous_lower = c.is_ascii_lowercase();
        label.push(c);
    }
    label
}

/// The value as the game shows it: "+1.7%" for percentage stats, "+2" or "-20" otherwise.
pub fn roll_text(stat: &str, value: i64) -> String {
    let sign = if value < 0 { "-" } else { "+" };
    let magnitude = value.unsigned_abs();
    if is_percent_stat(stat) {
        let (whole, tenths) = (magnitude / 10, magnitude % 10);
        if tenths == 0 {
            format!("{sign}{whole}%")
        } else {
            format!("{sign}{whole}.{tenths}%")
        }
    } else {
        format!("{sign}{magnitude}")
    }
}
