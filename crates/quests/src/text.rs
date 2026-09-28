//! Pure string helpers shared by the quest service and the packet handler: merchant/item display
//! name normalization (`quest_service.py`) and game-id prefix stripping (`quest_packet_handler.py`).

/// Merchant names the game sends with a suffix DnDTools has learned to fold into the base name
/// (e.g. event/daily variants), matched on the full lowercased, whitespace-collapsed name.
const MERCHANT_EXACT_ALIASES: &[(&str, &str)] = &[
    ("goblin merchant final", "Goblin Merchant"),
    ("huntress daily", "Huntress"),
    ("huntress daily equipment", "Huntress"),
    ("huntress seasonal", "Huntress"),
    ("huntress weekly", "Huntress"),
    ("krampus daily", "Krampus"),
    ("krampus seasonal", "Krampus"),
    ("tavern master final", "Tavern Master"),
    ("tavern master tuto", "Tavern Master"),
    ("the collector final", "The Collector"),
    ("valentine daily", "Valentine"),
    ("valentine seasonal", "Valentine"),
    ("weaponsmith extra", "Weaponsmith"),
];

/// Merchant name prefixes matched when no exact alias applies, in the order Python's `dict`
/// iterated them (first matching prefix wins).
const MERCHANT_PREFIX_ALIASES: &[(&str, &str)] = &[
    ("goblin merchant", "Goblin Merchant"),
    ("huntress", "Huntress"),
    ("krampus", "Krampus"),
    ("tavern master", "Tavern Master"),
    ("the collector", "The Collector"),
    ("valentine", "Valentine"),
    ("weaponsmith", "Weaponsmith"),
];

/// Collapse whitespace and fold known merchant name variants to their canonical display name.
/// Mirrors `QuestService._normalize_merchant_name`.
pub fn normalize_merchant_name(name: Option<&str>) -> String {
    let name = match name {
        Some(value) if !value.is_empty() => value,
        _ => return String::new(),
    };
    let cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = cleaned.to_lowercase();

    if let Some(&(_, canonical)) = MERCHANT_EXACT_ALIASES.iter().find(|(alias, _)| *alias == lowered) {
        return canonical.to_string();
    }
    if let Some(&(_, canonical)) =
        MERCHANT_PREFIX_ALIASES.iter().find(|(prefix, _)| lowered.starts_with(prefix))
    {
        return canonical.to_string();
    }
    cleaned
}

/// Turn a raw item id like `"LuckPotion_4001"` into a display name like `"Luck Potion 4001"`.
/// Mirrors `QuestService._normalize_item_name`.
pub fn normalize_item_name(item_id: &str) -> String {
    if item_id.is_empty() {
        return "Unknown Item".to_string();
    }
    let spaced = item_id.replace(['_', '-'], " ");
    let camel_split = insert_camel_case_boundaries(&spaced);
    let collapsed = camel_split.split_whitespace().collect::<Vec<_>>().join(" ");
    title_case(&collapsed)
}

/// Insert a space wherever a lowercase letter or digit is followed by an uppercase letter, the way
/// Python's `re.sub(r"(?<=[a-z0-9])(?=[A-Z])", " ", value)` does.
fn insert_camel_case_boundaries(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::with_capacity(value.len() + 4);
    for (index, &ch) in chars.iter().enumerate() {
        if index > 0 {
            let previous = chars[index - 1];
            let boundary = (previous.is_ascii_lowercase() || previous.is_ascii_digit()) && ch.is_ascii_uppercase();
            if boundary {
                result.push(' ');
            }
        }
        result.push(ch);
    }
    result
}

/// Title-case like Python's `str.title()`: the first letter of every alphabetic run is uppercased,
/// the rest of that run is lowercased, and non-alphabetic characters (spaces, digits) are word
/// boundaries left untouched.
fn title_case(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut in_word = false;
    for ch in value.chars() {
        if ch.is_alphabetic() {
            if in_word {
                result.extend(ch.to_lowercase());
            } else {
                result.extend(ch.to_uppercase());
            }
            in_word = true;
        } else {
            result.push(ch);
            in_word = false;
        }
    }
    result
}

/// Fully-qualified game ids the game's lobby messages use, e.g.
/// `"DesignDataQuest:Id_Quest_TavernMaster_Tuto_01"`, while DarkerDB (and this catalog) use the
/// short form `"TavernMaster_Tuto_01"`. Listed longest/most-specific first is not required since
/// each prefix targets a distinct id category.
const GAME_ID_PREFIXES: &[&str] = &[
    "DesignDataQuest:Id_Quest_",
    "DesignDataQuestChapter:Id_QuestChapter_",
    "DesignDataMerchant:Id_Merchant_",
    "DesignDataItem:Id_Item_",
    "DesignDataMonster:Id_Monster_",
    "DesignDataObject:Id_Object_",
];

/// Strip a known game-engine id prefix so packet ids match catalog ids. Mirrors
/// `quest_packet_handler._normalize_game_id`, including its generic `"...:Id_<Category>_"` fallback
/// for prefixes not in the known list.
pub fn normalize_game_id(raw_id: &str) -> String {
    if raw_id.is_empty() {
        return String::new();
    }
    for prefix in GAME_ID_PREFIXES {
        if let Some(stripped) = raw_id.strip_prefix(prefix) {
            return stripped.to_string();
        }
    }
    if let Some(marker) = raw_id.find(":Id_") {
        let after = &raw_id[marker + ":Id_".len()..];
        if let Some(underscore) = after.find('_') {
            return after[underscore + 1..].to_string();
        }
    }
    raw_id.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_merchant_name_folds_known_variants() {
        assert_eq!(normalize_merchant_name(Some("Huntress Weekly")), "Huntress");
        assert_eq!(normalize_merchant_name(Some("  Alchemist  ")), "Alchemist");
        assert_eq!(normalize_merchant_name(Some("")), "");
        assert_eq!(normalize_merchant_name(None), "");
    }

    #[test]
    fn normalize_item_name_splits_camel_case_and_separators() {
        assert_eq!(normalize_item_name("LuckPotion_4001"), "Luck Potion 4001");
        assert_eq!(normalize_item_name("GoldCoins"), "Gold Coins");
        assert_eq!(normalize_item_name(""), "Unknown Item");
    }

    #[test]
    fn normalize_game_id_strips_known_prefixes_and_generic_fallback() {
        assert_eq!(
            normalize_game_id("DesignDataQuest:Id_Quest_TavernMaster_Tuto_01"),
            "TavernMaster_Tuto_01"
        );
        assert_eq!(normalize_game_id("DesignDataMerchant:Id_Merchant_Alchemist"), "Alchemist");
        assert_eq!(normalize_game_id("SomethingElse:Id_Widget_Foo"), "Foo");
        assert_eq!(normalize_game_id("AlreadyShort"), "AlreadyShort");
        assert_eq!(normalize_game_id(""), "");
    }
}
