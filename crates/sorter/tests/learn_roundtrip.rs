//! Round-trip test (not a Python port): train the item model on synthetic events where the player
//! always moves potions to the bottom-left of the stash, save it, load it back into a fresh
//! `SortAdaptiveModel` instance, and confirm the loaded model still prefers bottom-left placements —
//! using the same `item_features` extraction the real sorter would use to score a candidate slot.

use std::collections::HashMap;

use game_data::{Item, Rarity};
use sorter::learn::{item_features, ItemFeatureInputs, SortAdaptiveModel, TrainingSample};
use state::OwnedItem;

/// One `ITEM_FEATURE_NAMES` vector for a 1x1 item at normalized position `(slot_x_norm,
/// slot_y_norm)`. Every feature except the two position ones is held constant across the synthetic
/// dataset, so position is the only signal the model has to learn from — the cleanest possible
/// setup for a test that must reliably converge.
fn potion_features_at(slot_x_norm: f64, slot_y_norm: f64) -> HashMap<String, f64> {
    HashMap::from([
        ("width".to_string(), 1.0),
        ("height".to_string(), 1.0),
        ("area".to_string(), 1.0),
        ("max_side".to_string(), 1.0),
        ("rarity".to_string(), 1.0),
        ("slot_x".to_string(), slot_x_norm * 11.0),
        ("slot_y".to_string(), slot_y_norm * 19.0),
        ("slot_x_norm".to_string(), slot_x_norm),
        ("slot_y_norm".to_string(), slot_y_norm),
        ("pack_mode".to_string(), 0.0),
        ("stack_mode".to_string(), 0.0),
        ("free_ratio".to_string(), 0.5),
        ("distance_from_current".to_string(), 0.0),
        ("blockers_at_target".to_string(), 0.0),
        ("neighbor_rarity_match".to_string(), 0.0),
    ])
}

fn potion_item() -> Item {
    Item {
        id: "Potion_1001".to_string(),
        name: "Potion".to_string(),
        rarity: Rarity::Common,
        item_type: "Utility".to_string(),
        slot_type: String::new(),
        hand_type: String::new(),
        weapon_type: String::new(),
        armor_type: String::new(),
        utility_type: String::new(),
        tradable: true,
        max_stack: 1,
        width: 1,
        height: 1,
        vendor_price: 0,
        icon_path: None,
    }
}

fn potion_owned() -> OwnedItem {
    OwnedItem {
        unique_id: 1,
        item_id: "Potion_1001".to_string(),
        count: 1,
        inventory_id: state::stash::STORAGE,
        slot_id: None,
        base: Vec::new(),
        rolls: Vec::new(),
        loot_state: 0,
        tradable: true,
        contents: 0,
    }
}

#[test]
fn a_model_trained_on_a_bottom_left_preference_still_prefers_it_after_a_save_and_load_round_trip() {
    // Arrange: the player always accepts potions placed toward the bottom-left of the stash (low
    // slot_x_norm, high slot_y_norm) and always corrects everything else away — recorded here as a
    // grid of labeled placement outcomes, the shape `SortEventStore::get_item_training_data` would
    // hand to `train_items` in the real app.
    let steps = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0];
    let mut events = Vec::new();
    for &x in &steps {
        for &y in &steps {
            events.push(TrainingSample { features: potion_features_at(x, y), label: y > x, weight: 1.0 });
        }
    }
    assert!(events.len() >= 30, "must clear MIN_ITEM_SAMPLES for training to actually run");

    let dir = tempfile::tempdir().expect("create temp dir for test model");
    let model = SortAdaptiveModel::new(dir.path());

    // Act: train synchronously so the test is deterministic, then persist by simply existing — the
    // model already saved itself to disk as a side effect of training (see `model.rs`'s
    // `save_payload`), so "save" needs no separate step here.
    model.train_items(events, false);
    let trained_version = model.item_version().expect("36 balanced samples must produce a trained model");

    // Build two real candidate feature vectors with the app's own `item_features`, one at the
    // stash's bottom-left corner and one at its top-right corner.
    let item = potion_item();
    let owned = potion_owned();
    let bottom_left = ItemFeatureInputs {
        catalog_item: &item,
        owned: &owned,
        inventory_id: state::stash::STORAGE,
        candidate_slot: (0, 19),
        pack_mode: false,
        stack_mode: false,
        free_cells: 120,
        total_cells: 240,
        occupied: &[],
    };
    let top_right = ItemFeatureInputs {
        catalog_item: &item,
        owned: &owned,
        inventory_id: state::stash::STORAGE,
        candidate_slot: (11, 0),
        pack_mode: false,
        stack_mode: false,
        free_cells: 120,
        total_cells: 240,
        occupied: &[],
    };
    let bottom_left_features = item_features(&bottom_left);
    let top_right_features = item_features(&top_right);

    // Assert: right after training, bottom-left already scores higher.
    let bottom_left_score = model.score_item_slot(&bottom_left_features).expect("item model should be trained");
    let top_right_score = model.score_item_slot(&top_right_features).expect("item model should be trained");
    assert!(
        bottom_left_score > top_right_score,
        "bottom-left ({bottom_left_score}) should outscore top-right ({top_right_score}) right after training"
    );

    // Act: load the model fresh from disk, as if the app had just restarted.
    let reloaded = SortAdaptiveModel::new(dir.path());

    // Assert: the round trip preserved both the model identity and the learned preference exactly.
    assert_eq!(reloaded.item_version(), Some(trained_version));
    let reloaded_bottom_left = reloaded.score_item_slot(&bottom_left_features).expect("reloaded item model should score");
    let reloaded_top_right = reloaded.score_item_slot(&top_right_features).expect("reloaded item model should score");
    assert_eq!(reloaded_bottom_left, bottom_left_score);
    assert_eq!(reloaded_top_right, top_right_score);
    assert!(reloaded_bottom_left > reloaded_top_right, "the bottom-left preference must survive the save/load round trip");
}
