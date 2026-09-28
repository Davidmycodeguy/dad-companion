//! Direct ports of the sort/search-related cases from DnDTools' `tests/test_stash_search.py` that
//! exercise this crate's public API (a few more are ported as unit tests alongside the code they
//! cover: `search_result_sort_key` in `src/plan/search_sort.rs`).
//!
//! Not ported: `test_equal_items_have_equal_hashes` and the `__hash__`/`__eq__` behavior it checks.
//! Python's `Item` hashes and compares by `(name, rarity, position)` only because it has no other
//! stable identity to use as a dict/set key; this port uses `OwnedItem::unique_id` — the game's own
//! stable id — as that key everywhere instead (see `World`'s doc comment), so there is no
//! equivalent "same name/rarity/position means equal" contract to preserve.

mod common;

use game_data::Rarity;
use sorter::plan::{compare_items, SortDirective, SortItem};

fn item(unique_id: u64, name: &str, rarity: Rarity) -> SortItem {
    SortItem {
        unique_id,
        item_id: name.to_string(),
        name: name.to_string(),
        rarity,
        slot_type: String::new(),
        width: 1,
        height: 1,
        quantity: 1,
        max_stack: 1,
    }
}

/// Port of `test_equal_sort_fields_do_not_use_process_memory_as_tiebreaker`: two items tied on
/// every sortable and fallback field compare equal — not by position, and not by any Rust-side
/// equivalent of Python's `id()` (there is none: nothing here ever breaks a tie by memory address
/// or insertion order), so a stable sort leaves their relative order exactly as given.
#[test]
fn equal_sort_fields_produce_no_ordering_preference() {
    let first = item(1, "Potion", Rarity::Common);
    let second = item(2, "Potion", Rarity::Common);

    assert_eq!(compare_items(&first, &second, &[]), std::cmp::Ordering::Equal);
    assert_eq!(compare_items(&second, &first, &[]), std::cmp::Ordering::Equal);
}

/// Port of `test_item_compare_parses_string_directives_inside_a_list`: a single `"rarity:asc"`
/// directive (parsed from the same string shape the Python API accepted) puts the lower rarity
/// first.
#[test]
fn a_parsed_rarity_asc_directive_orders_lower_rarity_first() {
    let common = item(1, "Blade", Rarity::Common);
    let epic = item(2, "Blade", Rarity::Epic);
    let directives: Vec<SortDirective> = ["rarity:asc"].iter().filter_map(|s| SortDirective::parse(s)).collect();

    assert_eq!(compare_items(&common, &epic, &directives), std::cmp::Ordering::Less);
}
