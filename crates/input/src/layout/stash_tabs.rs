//! Stash tab selectors: which `StashType` sits behind each of the 8 tab-selector positions.

/// Number of stash tab selectors in the game UI (Storage, Purchased 1-5, Shared Stash, Shared
/// Stash Seasonal). Ports `macros.py`'s `STASH_TAB_COUNT`.
pub const STASH_TAB_COUNT: usize = 8;

/// Default mapping: tab index (0-7) -> `StashType` value. Box 0 = Storage, box 1 = Shared Stash,
/// boxes 2-7 = Purchased 1-5 + Seasonal. Ports `macros.py`'s `DEFAULT_STASH_TAB_MAPPING`.
///
/// Note: `settings.py`'s own `_build_defaults` ships a *different* all-zero default
/// (`[0, 0, 0, 0, 0, 0, 0, 0]`) for a freshly-created settings file; this constant is the one
/// `macros.py` itself falls back to whenever the saved mapping is missing or malformed. Both are
/// Python-side (`appdata`/settings) concerns outside this crate; callers own picking the right one.
pub const DEFAULT_STASH_TAB_MAPPING: [i32; STASH_TAB_COUNT] = [4, 20, 5, 6, 7, 8, 9, 30];

/// Human-readable name for a `StashType` value, or `None` if it isn't one of the known stash
/// types. Ports `macros.py`'s `STASH_TYPE_NAMES`.
pub fn stash_type_name(stash_type: i32) -> Option<&'static str> {
    Some(match stash_type {
        4 => "Storage",
        5 => "Purchased 1",
        6 => "Purchased 2",
        7 => "Purchased 3",
        8 => "Purchased 4",
        9 => "Purchased 5",
        20 => "Shared Seasonal",
        30 => "Shared Stash",
        _ => return None,
    })
}

/// The tab index for `stash_type` under `mapping`, or `None` if no tab selector maps to it
/// (mirrors a `0`/absent entry in the Python mapping, e.g. for BAG or EQUIPMENT). Ports the
/// `STASH_TYPE_TO_TAB_INDEX` lookup dict, computed here on demand instead of cached as a global
/// rebuilt by `load_tab_mapping()`.
pub fn tab_index_for_stash_type(mapping: &[i32], stash_type: i32) -> Option<usize> {
    mapping.iter().position(|&value| value == stash_type && value != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mapping_matches_python() {
        assert_eq!(DEFAULT_STASH_TAB_MAPPING, [4, 20, 5, 6, 7, 8, 9, 30]);
    }

    #[test]
    fn stash_type_name_covers_all_default_entries() {
        for value in DEFAULT_STASH_TAB_MAPPING {
            assert!(stash_type_name(value).is_some(), "missing name for {value}");
        }
        assert_eq!(stash_type_name(999), None);
    }

    #[test]
    fn tab_index_finds_first_match_and_skips_zero_entries() {
        let mapping = DEFAULT_STASH_TAB_MAPPING;
        assert_eq!(tab_index_for_stash_type(&mapping, 4), Some(0));
        assert_eq!(tab_index_for_stash_type(&mapping, 5), Some(2));
        assert_eq!(tab_index_for_stash_type(&mapping, 30), Some(7));
        assert_eq!(tab_index_for_stash_type(&mapping, 2), None);

        let with_gap = [4, 20, 5, 6, 7, 8, 0, 30];
        assert_eq!(tab_index_for_stash_type(&with_gap, 9), None);
    }
}
