//! "Train now": retrain the value model on everything the market history holds, check its accuracy
//! on held-out listings, save it, and put it in use at once. Also the learned roll patterns (which
//! stat pairs sell for more together) that pricing reads.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use market::{TrainOptions, WorthModel};

use crate::state::{AppState, MARKET_DB, WORTH_MODEL};

/// Where the learned roll patterns are saved (DnDTools' `market_model.json`).
pub const MARKET_MODEL: &str = "market_model.json";

/// What pricing learned from the market beyond the value model: bonuses for stat pairs that sell
/// for more together, and how much of an extra good roll's premium buyers pay.
#[derive(Debug, Clone)]
pub struct Learned {
    pub synergies: HashMap<BTreeSet<String>, f64>,
    pub extra_share: f64,
}

impl Learned {
    /// The patterns saved in `path`; the defaults when the file is missing or unreadable.
    pub fn load(path: &Path) -> Self {
        let model = market::patterns::load_model(path);
        Learned {
            synergies: market::pair_bonuses(&model, market::patterns::MIN_PAIR_SUPPORT),
            extra_share: market::extra_roll_share(&model, market::EXTRA_ROLL_SHARE),
        }
    }
}

/// The saved model's median error on held-out listings, as a share (DnDTools saved percent).
pub fn saved_error(path: &Path) -> Option<f64> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("evaluation")?.get("model_mdape")?.as_f64().map(|percent| percent / 100.0)
}

/// Each item's pricing group, as DnDTools trained with: its gear slot, else its item type, else
/// "other" (also for items the catalog doesn't know).
pub fn pricing_groups<'a>(catalog: &game_data::ItemCatalog, item_ids: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    let mut groups = HashMap::new();
    for item_id in item_ids {
        if groups.contains_key(item_id) {
            continue;
        }
        let item = catalog.get(item_id);
        let group = item
            .map(|i| i.slot_type.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| item.map(|i| i.item_type.as_str()).filter(|t| !t.is_empty()))
            .unwrap_or("other");
        groups.insert(item_id.to_owned(), group.to_owned());
    }
    groups
}

/// Re-learns which stat pairs sell for more together (and similar patterns) from every saved
/// listing, saves them as `market_model.json` and puts them in use for pricing.
pub fn analyze_market(state: &AppState) -> Result<(), String> {
    let Some(market) = state.market.as_ref() else { return Err("Market data is unavailable.".into()) };
    let listings = market.pattern_listings().map_err(|err| err.to_string())?;
    if listings.is_empty() {
        return Ok(());
    }
    let mut item_types = HashMap::new();
    for listing in &listings {
        if item_types.contains_key(&listing.item_id) {
            continue;
        }
        let item = state.catalog.get(&listing.item_id);
        let kind = item.map(|i| i.item_type.as_str()).filter(|t| !t.is_empty()).unwrap_or("other");
        item_types.insert(listing.item_id.clone(), kind.to_owned());
    }
    let report = market::analyze(&listings, &item_types);
    let path = state.data.data().join(MARKET_MODEL);
    market::save_model(&path, &market::model_from_report(&report)).map_err(|err| format!("saving market patterns: {err}"))?;
    state.set_learned(Learned::load(&path));
    Ok(())
}

/// What one training run did.
#[derive(Debug, Clone, Copy)]
pub struct TrainSummary {
    pub listings: u64,
    /// Median error on held-out listings, as a share.
    pub error: f64,
}

/// Trains, evaluates, saves and swaps in a new value model. Takes a few seconds: call it off the
/// main thread.
pub fn train_worth_model(state: &AppState) -> Result<TrainSummary, String> {
    let listings = market::load_listings(&state.data.data().join(MARKET_DB)).map_err(|err| format!("reading market data: {err}"))?;
    if listings.is_empty() {
        return Err("There is no market data to learn from yet: browse the Marketplace or update market data first.".into());
    }
    let slots = pricing_groups(&state.catalog, listings.iter().map(|listing| listing.item_id.as_str()));
    let evaluation = market::evaluate_default(&listings, &slots, TrainOptions::default()).map_err(|err| err.to_string())?;
    let trained = market::train(&listings, &slots, TrainOptions::default()).map_err(|err| err.to_string())?;

    let mut json = trained.to_json();
    json["evaluation"] = serde_json::json!({
        "tested": evaluation.tested,
        "trained": evaluation.trained,
        "model_mdape": evaluation.model_mdape,
        "model_within_25": evaluation.model_within_25,
        "baseline_mdape": evaluation.baseline_mdape,
        "baseline_within_25": evaluation.baseline_within_25,
    });
    let text = serde_json::to_string(&json).map_err(|err| err.to_string())?;
    let model = WorthModel::from_json(&text).map_err(|err| format!("the new model could not be read back: {err}"))?;

    // Written whole, then renamed, so a crash never leaves half a model on disk.
    let path = state.data.data().join(WORTH_MODEL);
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, &text).and_then(|()| std::fs::rename(&partial, &path)).map_err(|err| format!("saving the model: {err}"))?;

    let summary = TrainSummary { listings: model.listings(), error: evaluation.model_mdape / 100.0 };
    state.set_worth(model, Some(summary.error));
    log::info!("value model trained on {} listings, median error {:.1}%", summary.listings, evaluation.model_mdape);
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::pricing_groups;

    #[test]
    fn items_group_by_slot_then_type_then_other() {
        let catalog = game_data::ItemCatalog::from_json(
            r#"{"Robe": {"id": "Robe", "name": "Robe", "rarity": "Rare", "type": "Armor", "slot_type": "Chest"},
                "Gem": {"id": "Gem", "name": "Gem", "rarity": "Rare", "type": "Gem"},
                "Junk": {"id": "Junk", "name": "Junk", "rarity": "Common"}}"#,
        )
        .unwrap();
        let groups = pricing_groups(&catalog, ["Robe", "Gem", "Junk", "Unknown", "Robe"].into_iter());
        assert_eq!(groups["Robe"], "Chest");
        assert_eq!(groups["Gem"], "Gem");
        assert_eq!(groups["Junk"], "other");
        assert_eq!(groups["Unknown"], "other");
        assert_eq!(groups.len(), 4);
    }
}
