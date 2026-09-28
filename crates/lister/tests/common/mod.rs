//! Shared helpers for the `lister` integration tests. Not itself a test binary (Cargo only treats
//! direct files under `tests/` that way); each test file pulls this in with `mod common;`.

use serde_json::{json, Value};

/// A stash item as the game/capture layer hands it to the lister: same defaults as the Python
/// tests' `_item(uid, slot, **kw)` helper, with `overrides` (a JSON object) applied on top.
pub fn item(uid: &str, slot: i64, overrides: Value) -> Value {
    let Value::Object(mut map) = json!({
        "name": format!("Item {uid}"), "itemId": format!("Id_{uid}"), "itemUniqueId": uid, "slotId": slot,
        "itemCount": 1, "rarity": 5, "width": 1, "height": 1, "pp": [], "sp": [],
        "vendor_price": 10, "max_stack_size": 1,
    }) else {
        unreachable!("the literal above is always a JSON object")
    };
    if let Value::Object(over) = overrides {
        map.extend(over);
    }
    Value::Object(map)
}
