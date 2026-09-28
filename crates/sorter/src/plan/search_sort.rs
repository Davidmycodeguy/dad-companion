//! Stable ordering for cross-character item-search results. Port of `search_sort.py`.
//!
//! This lives under `plan` (the file scope for this port) but is otherwise independent of the
//! rest of the module: it orders search hits, not stash layouts.

use std::cmp::Reverse;

use game_data::Rarity;

use super::item::rarity_rank;

/// One search hit, as far as ordering cares. Port of the dict shape `search_sort.py` reads
/// (`result["item"]["name"/"rarity"/"pp"/"sp"]`, `result["nickname"/"id"/"stash_id"/"slotId"]`).
#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    pub name: String,
    pub rarity: Rarity,
    /// Base stats as displayed: `(label, value)` pairs.
    pub pp: Vec<(String, String)>,
    /// Rolled stats as displayed: `(label, value)` pairs.
    pub sp: Vec<(String, String)>,
    pub nickname: String,
    pub id: String,
    pub stash_id: String,
    pub slot_id: String,
}

/// Either a parsed integer or, when a value doesn't parse as one, its lowercased text — integers
/// always sort first. Port of `_numeric_then_text`; ids like `stash_id`/`slotId` are numeric in
/// practice but arrive as strings, so a plain string sort would put `"20"` before `"4"`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum NumericOrText {
    Numeric(i64),
    Text(String),
}

fn numeric_then_text(value: &str) -> NumericOrText {
    match value.trim().parse::<i64>() {
        Ok(n) => NumericOrText::Numeric(n),
        Err(_) => NumericOrText::Text(casefold(value)),
    }
}

/// Rust has no built-in Unicode casefold; lowercasing is the closest without a new dependency and
/// matches Python's `.casefold()` for the ASCII names/ids this sorts in practice.
fn casefold(value: &str) -> String {
    value.to_lowercase()
}

/// A comparable, fully-owned sort key for one [`SearchResult`]. Port of the tuple
/// `search_result_sort_key` returns; Rust needs a named type to `#[derive(Ord)]` on, since it (like
/// Python's tuple comparison) compares field by field in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SearchResultKey {
    name: String,
    rarity_desc: Reverse<u8>,
    pp: Vec<(String, String)>,
    sp: Vec<(String, String)>,
    nickname: String,
    id: String,
    stash_id: NumericOrText,
    slot_id: NumericOrText,
}

/// A deterministic, user-facing key for a stash search result: by item name, then rarity
/// (highest first), then base and rolled stats, then who owns it, then where. Port of
/// `search_result_sort_key`. Sort a slice with
/// `results.sort_by_cached_key(search_result_sort_key)`.
pub fn search_result_sort_key(result: &SearchResult) -> SearchResultKey {
    SearchResultKey {
        name: casefold(&result.name),
        rarity_desc: Reverse(rarity_rank(result.rarity)),
        pp: result.pp.iter().map(|(k, v)| (casefold(k), casefold(v))).collect(),
        sp: result.sp.iter().map(|(k, v)| (casefold(k), casefold(v))).collect(),
        nickname: casefold(&result.nickname),
        id: casefold(&result.id),
        stash_id: numeric_then_text(&result.stash_id),
        slot_id: numeric_then_text(&result.slot_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(name: &str, rarity: Rarity, nickname: &str, stash_id: &str, slot_id: &str) -> SearchResult {
        SearchResult {
            name: name.to_string(),
            rarity,
            nickname: nickname.to_string(),
            id: nickname.to_lowercase().replace(' ', "-"),
            stash_id: stash_id.to_string(),
            slot_id: slot_id.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn search_results_are_stable_and_not_capture_time_order() {
        // Deliberately the opposite of the expected display order: character-cache order follows
        // file mtimes and must not leak into API result order.
        let mut results = [
            result("Zebra Weapon", Rarity::Common, "Newest Capture", "20", "8"),
            result("Alpha Weapon", Rarity::Poor, "Newest Capture", "4", "9"),
            result("Alpha Weapon", Rarity::Epic, "Older Capture", "2", "1"),
        ];
        results.sort_by_cached_key(search_result_sort_key);
        let order: Vec<(&str, Rarity, &str)> =
            results.iter().map(|r| (r.name.as_str(), r.rarity, r.nickname.as_str())).collect();
        assert_eq!(
            order,
            vec![
                ("Alpha Weapon", Rarity::Epic, "Older Capture"),
                ("Alpha Weapon", Rarity::Poor, "Newest Capture"),
                ("Zebra Weapon", Rarity::Common, "Newest Capture"),
            ]
        );
    }

    #[test]
    fn tiebreaks_numeric_stash_and_slot_ids_instead_of_lexically() {
        let mut results = [
            result("Potion", Rarity::Common, "Hero", "20", "12"),
            result("Potion", Rarity::Common, "Hero", "4", "9"),
            result("Potion", Rarity::Common, "Hero", "4", "2"),
        ];
        results.sort_by_cached_key(search_result_sort_key);
        let order: Vec<(&str, &str)> =
            results.iter().map(|r| (r.stash_id.as_str(), r.slot_id.as_str())).collect();
        assert_eq!(order, vec![("4", "2"), ("4", "9"), ("20", "12")]);
    }
}
