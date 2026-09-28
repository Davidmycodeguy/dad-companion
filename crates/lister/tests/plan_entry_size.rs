//! Entries sent back from the page are checked against the sizes items really have.

use lister::plan::PlanEntry;
use serde_json::json;

fn entry(width: i64, height: i64) -> serde_json::Value {
    json!({"unique_id": "7", "name": "Quarterstaff", "rarity": 3, "stash_id": "2", "slot_id": 9, "width": width, "height": height, "price": 128})
}

#[test]
fn a_five_cell_tall_staff_is_a_valid_entry() {
    let staff = PlanEntry::from_dict(&entry(1, 5), false).expect("a 1x5 quarterstaff is a real item size");
    assert_eq!((staff.width, staff.height), (1, 5));
}

#[test]
fn sizes_no_item_has_are_refused() {
    assert!(PlanEntry::from_dict(&entry(1, 6), false).is_err());
    assert!(PlanEntry::from_dict(&entry(5, 1), false).is_err());
}
