//! Plain wording for objectives and rewards, and the small name helpers the views share.

use game_data::Rarity;

use crate::catalog::{Objective, Reward};
use crate::mission_match::split_monster;

/// The rarity name a quest's rarity word stands for ("Legend" is "Legendary"); None when it isn't one.
pub(crate) fn rarity_word(raw: Option<&str>) -> Option<Rarity> {
    let rarity = Rarity::from_name(raw?);
    (rarity != Rarity::Unknown).then_some(rarity)
}

/// The words the game adds to each grade of gems and valuables ("Diamond (Exquisite)").
const GRADE_WORDS: [&str; 7] = ["Cracked", "Flawed", "Normal", "Exquisite", "Perfect", "Royal", "Ultimate"];

/// An item name without its grade word: "Diamond (Exquisite)" is "Diamond".
pub(crate) fn without_grade(name: &str) -> &str {
    match name.strip_suffix(')').and_then(|rest| rest.rsplit_once(" (")) {
        Some((base, word)) if GRADE_WORDS.contains(&word) => base,
        _ => name,
    }
}

/// A merchant name reduced to letters and digits, lowercase: "Tavern Master" and the game's
/// "TavernMaster" both become "tavernmaster".
pub fn compact_name(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// "Daily", "Weekly" or "Seasonal" for quests that rotate, from the catalog's merchant variant
/// ("Huntress Daily") or the quest id ("Huntress_Weekly_27").
pub(crate) fn time_limit(merchant: &str, quest_id: &str) -> Option<&'static str> {
    let text = format!("{merchant} {quest_id}").to_ascii_lowercase();
    if text.contains("daily") {
        Some("Daily")
    } else if text.contains("weekly") {
        Some("Weekly")
    } else if text.contains("season") {
        Some("Seasonal")
    } else {
        None
    }
}

/// A monster as a player reads it: "Id.character.undead Skeleton" is "Skeleton", a bare family
/// ("Id.character.undead") is "undead".
pub(crate) fn monster_name(raw: &str) -> String {
    let (words, family) = split_monster(raw);
    if words.is_empty() {
        family.unwrap_or_default()
    } else {
        words
    }
}

/// A dungeon place without the catalog's variant numbers: "Ruins Great Hall 03 Destroyed" is
/// "Ruins Great Hall Destroyed".
pub(crate) fn place_name(raw: &str) -> String {
    raw.split_whitespace().filter(|word| !word.bytes().all(|b| b.is_ascii_digit())).collect::<Vec<_>>().join(" ")
}

/// Words from a dotted game id: "id.item_skin.ceremonial_torch" is "Ceremonial Torch".
pub(crate) fn id_words(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    last.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What an objective asks, in plain words. `item_name` names its item (Fetch and Use Item);
/// `place` is the dungeon a Survive objective is about.
pub(crate) fn objective_label(objective: &Objective, item_name: Option<&str>, place: Option<&str>) -> String {
    let count = objective.count.unwrap_or(0);
    let rarity = rarity_word(objective.rarity.as_deref()).map(|r| format!("{} ", r.name())).unwrap_or_default();
    match objective.kind.as_str() {
        "Fetch" => match (item_name, objective.item_type.as_deref()) {
            (Some(name), _) => format!("Bring {count} {rarity}{name}"),
            (None, Some(item_type)) => format!("Bring {count} {rarity}{item_type}"),
            (None, None) => format!("Bring {count} {rarity}items"),
        },
        "Use Item" => format!("Use {count} {rarity}{}", item_name.unwrap_or("items")),
        "Kill" => {
            let monster = objective.monster.as_deref().map(monster_name).filter(|m| !m.is_empty());
            format!("Kill {count} {}", monster.as_deref().unwrap_or("monsters"))
        }
        "Explore" => match objective.module.as_deref().map(place_name).filter(|p| !p.is_empty()) {
            Some(module) => format!("Explore {module}"),
            None => "Explore".to_string(),
        },
        "Props" => props_label(objective.interact.as_deref().unwrap_or("objects"), count),
        "Survive" => {
            let times = if count > 1 { format!(" {count} times") } else { String::new() };
            match place {
                Some(place) => format!("Escape from {place}{times}"),
                None => format!("Escape from a dungeon{times}"),
            }
        }
        "Damage" => format!("Deal {count} damage"),
        other => other.to_string(),
    }
}

fn props_label(interact: &str, count: u32) -> String {
    match interact.strip_prefix("Destroy ") {
        Some(target) if count > 1 => format!("Destroy {count} {target}"),
        Some(_) => interact.to_string(),
        None if count > 1 => format!("Interact with {count} {interact}"),
        None => format!("Interact with {interact}"),
    }
}

/// A reward in plain words, without its count ("Surgical Kit", "Random Rare Armor").
pub(crate) fn reward_label(reward: &Reward, item_name: Option<&str>) -> String {
    match reward.kind.as_str() {
        "Item" => item_name.map(str::to_string).unwrap_or_else(|| "Item".to_string()),
        "Experience" => "Experience".to_string(),
        "Affinity" => match reward.merchant.as_deref() {
            Some(merchant) => format!("{merchant} affinity"),
            None => "Affinity".to_string(),
        },
        "Random" => {
            let rarity = rarity_word(reward.rarity.as_deref()).map(|r| format!("{} ", r.name())).unwrap_or_default();
            format!("Random {rarity}{}", reward.item_type.as_deref().unwrap_or("reward"))
        }
        "Item Skin" => skin_label(reward.item_skin_id.as_deref(), "Item skin"),
        "Emote" | "Lobby Emote" => skin_label(reward.emote_id.as_deref(), &reward.kind),
        "Action" => skin_label(reward.action_skin_id.as_deref(), "Action"),
        other => other.to_string(),
    }
}

fn skin_label(id: Option<&str>, kind: &str) -> String {
    match id.map(id_words).filter(|words| !words.is_empty()) {
        Some(words) => format!("{kind}: {words}"),
        None => kind.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn objective(value: serde_json::Value) -> Objective {
        serde_json::from_value(value).expect("valid objective")
    }
    fn reward(value: serde_json::Value) -> Reward {
        serde_json::from_value(value).expect("valid reward")
    }

    #[test]
    fn objectives_read_plainly() {
        let rare = objective(json!({"type": "Fetch", "count": 2, "item_id": "Diamond", "rarity": "Rare"}));
        assert_eq!(objective_label(&rare, Some("Diamond"), None), "Bring 2 Rare Diamond");
        let armor = objective(json!({"type": "Fetch", "count": 1, "item_id": null, "rarity": "Legend", "item_type": "Armor"}));
        assert_eq!(objective_label(&armor, None, None), "Bring 1 Legendary Armor");
        let kill = objective(json!({"type": "Kill", "count": 3, "monster": "Id.character.undead Skeleton"}));
        assert_eq!(objective_label(&kill, None, None), "Kill 3 Skeleton");
        let undead = objective(json!({"type": "Kill", "count": 1, "monster": "Id.character.undead"}));
        assert_eq!(objective_label(&undead, None, None), "Kill 1 undead");
        let explore = objective(json!({"type": "Explore", "count": 1, "module": "Ruins Great Hall 03 Destroyed"}));
        assert_eq!(objective_label(&explore, None, None), "Explore Ruins Great Hall Destroyed");
        let pots = objective(json!({"type": "Props", "count": 5, "interact": "Destroy Spider Pot"}));
        assert_eq!(objective_label(&pots, None, None), "Destroy 5 Spider Pot");
        let torch = objective(json!({"type": "Props", "count": 1, "interact": "Torch"}));
        assert_eq!(objective_label(&torch, None, None), "Interact with Torch");
        let escape = objective(json!({"type": "Survive", "count": 6}));
        assert_eq!(objective_label(&escape, None, Some("Crypts")), "Escape from Crypts 6 times");
        assert_eq!(objective_label(&objective(json!({"type": "Survive", "count": 1})), None, None), "Escape from a dungeon");
        let damage = objective(json!({"type": "Damage", "count": 25}));
        assert_eq!(objective_label(&damage, None, None), "Deal 25 damage");
        let used = objective(json!({"type": "Use Item", "count": 3, "item_id": "Bandage"}));
        assert_eq!(objective_label(&used, Some("Bandage"), None), "Use 3 Bandage");
    }

    #[test]
    fn rewards_read_plainly() {
        let random = reward(json!({"type": "Random", "count": 1, "item_type": "Armor", "rarity": "Uncommon"}));
        assert_eq!(reward_label(&random, None), "Random Uncommon Armor");
        let affinity = reward(json!({"type": "Affinity", "count": 10, "merchant": "Alchemist"}));
        assert_eq!(reward_label(&affinity, None), "Alchemist affinity");
        let skin = reward(json!({"type": "Item Skin", "count": 1, "item_skin_id": "id.item_skin.ceremonial_torch"}));
        assert_eq!(reward_label(&skin, None), "Item skin: Ceremonial Torch");
        let item = reward(json!({"type": "Item", "count": 1, "item_id": "SurgicalKit_2001"}));
        assert_eq!(reward_label(&item, Some("Surgical Kit")), "Surgical Kit");
        assert_eq!(reward_label(&reward(json!({"type": "Stash", "count": 1})), None), "Stash");
    }

    #[test]
    fn names_and_rotations() {
        assert_eq!(compact_name("Tavern Master"), compact_name("TavernMaster"));
        assert_eq!(compact_name("Jack O Lantern"), "jackolantern");
        assert_eq!(time_limit("Huntress Daily", "Huntress_Daily_30"), Some("Daily"));
        assert_eq!(time_limit("Huntress Weekly", "Huntress_Weekly_27"), Some("Weekly"));
        assert_eq!(time_limit("Valentine Seasonal", "Valentine_Seasonal_01"), Some("Seasonal"));
        assert_eq!(time_limit("Alchemist", "Alchemist_01"), None);
        assert_eq!(rarity_word(Some("Legend")), Some(Rarity::Legendary));
        assert_eq!(rarity_word(Some("shiny")), None);
        assert_eq!(without_grade("Diamond (Exquisite)"), "Diamond");
        assert_eq!(without_grade("Gem Necklace (Normal)"), "Gem Necklace");
        assert_eq!(without_grade("Bandage"), "Bandage");
        assert_eq!(without_grade("Cloak (Hooded)"), "Cloak (Hooded)");
    }
}
