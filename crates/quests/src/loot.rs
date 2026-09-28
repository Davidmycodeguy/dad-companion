//! Loot-state requirements for quest items: how the game says the player must have obtained an
//! item (looted, crafted, supplied, ...) for it to count toward a quest. Ported from
//! `src/models/loot.py`, which several `quest_service.py` tests import directly.
//!
//! Objectives here are read as loosely-typed JSON (`serde_json::Value`), mirroring the Python's use
//! of plain dicts: this data does not come from the `assets/quests.json` catalog (whose objectives
//! never carry a loot-state field) but from whatever quest source the app feeds in, so a fixed
//! struct would be more rigid than the source it replaces.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// The game never restricts an item's source ("any loot state is fine").
pub const LOOT_STATE_NONE: i32 = 0;
/// The item must have been supplied by another player or NPC.
pub const LOOT_STATE_SUPPLIED: i32 = 1;
/// The item must have been looted from the world or a monster.
pub const LOOT_STATE_LOOTED: i32 = 2;
/// The item must have been handled (e.g. picked up and processed) in a specific way.
pub const LOOT_STATE_HANDLED: i32 = 3;
/// The item must have been crafted.
pub const LOOT_STATE_CRAFTED: i32 = 4;
/// The item must have come from an ally.
pub const LOOT_STATE_ALLY: i32 = 5;

/// Every alias `_parse_single_loot_state` accepted, flattened from the two-step table the Python
/// module builds (base value/label pairs, then a handful of extra synonyms).
const LOOT_STATE_ALIASES: &[(&str, i32)] = &[
    ("0", LOOT_STATE_NONE),
    ("none", LOOT_STATE_NONE),
    ("none_source", LOOT_STATE_NONE),
    ("nonesource", LOOT_STATE_NONE),
    ("1", LOOT_STATE_SUPPLIED),
    ("supplied", LOOT_STATE_SUPPLIED),
    ("supplies", LOOT_STATE_SUPPLIED),
    ("2", LOOT_STATE_LOOTED),
    ("looted", LOOT_STATE_LOOTED),
    ("loot", LOOT_STATE_LOOTED),
    ("3", LOOT_STATE_HANDLED),
    ("handled", LOOT_STATE_HANDLED),
    ("handle", LOOT_STATE_HANDLED),
    ("4", LOOT_STATE_CRAFTED),
    ("crafted", LOOT_STATE_CRAFTED),
    ("craft", LOOT_STATE_CRAFTED),
    ("5", LOOT_STATE_ALLY),
    ("ally", LOOT_STATE_ALLY),
];

/// The label the game/UI uses for a loot-state value, or the number itself if it's unrecognized.
pub fn format_loot_state_label(value: i32) -> String {
    match value {
        LOOT_STATE_NONE => "None",
        LOOT_STATE_SUPPLIED => "Supplied",
        LOOT_STATE_LOOTED => "Looted",
        LOOT_STATE_HANDLED => "Handled",
        LOOT_STATE_CRAFTED => "Crafted",
        LOOT_STATE_ALLY => "Ally",
        _ => return value.to_string(),
    }
    .to_string()
}

/// Lowercase, collapse separators to spaces, and drop the word "only" — the same cleanup
/// `_normalize_loot_state_key` applies before an alias lookup.
fn normalize_loot_state_key(raw: &str) -> String {
    let cleaned = raw.trim().to_lowercase();
    if cleaned.is_empty() {
        return String::new();
    }
    let cleaned = cleaned.replace(['-', '_'], " ").replace("only", "");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse one loot-state value: a JSON integer is used as-is; a string is normalized and matched
/// against [`LOOT_STATE_ALIASES`]; anything else (or an unrecognized string) yields `None`.
fn parse_single_loot_state(raw: &Value) -> Option<i32> {
    if let Some(number) = raw.as_i64() {
        return Some(number as i32);
    }
    let text = raw.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let key = normalize_loot_state_key(text);
    if key.is_empty() || matches!(key.as_str(), "any" | "all" | "*") {
        return None;
    }
    LOOT_STATE_ALIASES.iter().find(|(alias, _)| *alias == key).map(|&(_, value)| value)
}

/// The acceptable loot-state values for one objective's `loot_state` field, or `None` when
/// unrestricted (missing, null, or an unrecognized value). A JSON array is the union of each
/// entry's values, unless any entry is itself unrestricted — then the whole array is.
fn extract_loot_state_filter(raw: &Value) -> Option<HashSet<i32>> {
    match raw {
        Value::Null => None,
        Value::Array(entries) => {
            let mut collected = HashSet::new();
            for entry in entries {
                collected.extend(extract_loot_state_filter(entry)?);
            }
            if collected.is_empty() {
                None
            } else {
                Some(collected)
            }
        }
        scalar => parse_single_loot_state(scalar).map(|value| HashSet::from([value])),
    }
}

/// Map each quest-objective item id to the loot-state values it restricts to, across every
/// objective in `quests` that names it. `None` means at least one objective left that item
/// unrestricted, which always wins over a restriction seen elsewhere (mirrors
/// `collect_item_loot_state_requirements`: once an item is recorded unrestricted, later
/// restrictions for the same item are ignored rather than narrowing it back down).
pub fn collect_item_loot_state_requirements(
    quests: &[Value],
    item_filter: Option<&HashSet<String>>,
) -> HashMap<String, Option<HashSet<i32>>> {
    let mut requirements: HashMap<String, Option<HashSet<i32>>> = HashMap::new();

    for quest in quests {
        let Some(objectives) = quest.get("objectives").and_then(Value::as_array) else { continue };
        for objective in objectives {
            let Some(item_id) = objective.get("item_id").and_then(Value::as_str) else { continue };
            if item_id.is_empty() {
                continue;
            }
            if item_filter.is_some_and(|filter| !filter.contains(item_id)) {
                continue;
            }

            let snake_case = objective.get("loot_state").filter(|value| !value.is_null());
            let raw = snake_case.or_else(|| objective.get("lootState")).unwrap_or(&Value::Null);

            match extract_loot_state_filter(raw) {
                None => {
                    requirements.insert(item_id.to_string(), None);
                }
                Some(values) if values.is_empty() => {}
                Some(values) => match requirements.get_mut(item_id) {
                    Some(Some(existing)) => existing.extend(values),
                    Some(None) => {}
                    None => {
                        requirements.insert(item_id.to_string(), Some(values));
                    }
                },
            }
        }
    }

    requirements
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numeric_zero_loot_state_remains_a_restricted_requirement() {
        let quests = vec![json!({
            "objectives": [
                {"item_id": "quest-item", "loot_state": 0},
            ]
        })];

        let requirements = collect_item_loot_state_requirements(&quests, None);

        assert_eq!(requirements.get("quest-item"), Some(&Some(HashSet::from([0]))));
    }

    #[test]
    fn none_snake_case_loot_state_falls_back_to_camel_case_value() {
        let quests = vec![json!({
            "objectives": [
                {"item_id": "quest-item", "loot_state": null, "lootState": 2},
            ]
        })];

        let requirements = collect_item_loot_state_requirements(&quests, None);

        assert_eq!(requirements.get("quest-item"), Some(&Some(HashSet::from([2]))));
    }
}
