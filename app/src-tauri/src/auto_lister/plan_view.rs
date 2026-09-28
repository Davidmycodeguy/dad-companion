//! The plan as the page sees it (camelCase JSON), and back. Entries the page sends to price or
//! list are checked with the lister's own validation, and their fees recomputed from the price.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use lister::plan::{Plan, PlanEntry};
use market::Skip;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub unique_id: String,
    pub name: String,
    pub rarity: i64,
    pub stash_id: String,
    pub slot_id: i64,
    pub width: i64,
    pub height: i64,
    pub price: i64,
    pub fee: i64,
    pub vendor_price: i64,
    pub item_id: String,
    pub base_rolls: Vec<(String, i64)>,
    pub rolls: Vec<(String, i64)>,
    pub flag: String,
    pub compared: String,
    pub confidence: String,
    pub quantity: i64,
    pub recommended: i64,
}

impl From<&PlanEntry> for EntryView {
    fn from(e: &PlanEntry) -> Self {
        EntryView {
            unique_id: e.unique_id.clone(),
            name: e.name.clone(),
            rarity: e.rarity,
            stash_id: e.stash_id.clone(),
            slot_id: e.slot_id,
            width: e.width,
            height: e.height,
            price: e.price,
            fee: e.fee,
            vendor_price: e.vendor_price,
            item_id: e.item_id.clone(),
            base_rolls: e.base_rolls.clone(),
            rolls: e.rolls.clone(),
            flag: e.flag.clone(),
            compared: e.compared.clone(),
            confidence: e.confidence.clone(),
            quantity: e.quantity,
            recommended: e.recommended,
        }
    }
}

impl EntryView {
    /// The lister's entry, validated like any untrusted entry; `allow_unpriced` permits price 0.
    pub fn into_entry(self, allow_unpriced: bool) -> Result<PlanEntry, String> {
        let dict = json!({
            "unique_id": self.unique_id, "name": self.name, "rarity": self.rarity, "stash_id": self.stash_id,
            "slot_id": self.slot_id, "width": self.width, "height": self.height, "price": self.price,
            "vendor_price": self.vendor_price, "item_id": self.item_id, "base_rolls": self.base_rolls,
            "rolls": self.rolls, "flag": self.flag, "compared": self.compared, "confidence": self.confidence,
            "quantity": self.quantity, "recommended": self.recommended,
        });
        PlanEntry::from_dict(&dict, allow_unpriced).map_err(|err| format!("{}: {err}", self.name))
    }
}

/// Most items one run may take: every listing spot an account has.
pub const MAX_RUN_ITEMS: usize = 40;

/// Checks the entries the page sent: at least one, at most a run's worth, each valid, no repeats.
pub fn parse_entries(
    entries: Vec<EntryView>,
    verb: &str,
    allow_unpriced: bool,
) -> Result<Vec<PlanEntry>, String> {
    if entries.is_empty() {
        return Err(format!("Nothing to {verb}."));
    }
    if entries.len() > MAX_RUN_ITEMS {
        return Err(format!("At most {MAX_RUN_ITEMS} items at once."));
    }
    let mut seen = std::collections::HashSet::new();
    let mut parsed = Vec::new();
    for view in entries {
        let entry = view.into_entry(allow_unpriced)?;
        if seen.insert(entry.unique_id.clone()) {
            parsed.push(entry);
        }
    }
    Ok(parsed)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipView {
    name: String,
    stash_id: String,
    slot_id: i64,
    reason: String,
    flag: String,
    confidence: String,
    unique_id: String,
    merchant: bool,
}

impl From<&Skip> for SkipView {
    fn from(s: &Skip) -> Self {
        SkipView {
            name: s.name.clone(),
            stash_id: s.stash_id.clone(),
            slot_id: s.slot_id,
            reason: s.reason.clone(),
            flag: s.flag.clone(),
            confidence: s.confidence.clone(),
            unique_id: s.unique_id.clone(),
            merchant: market::is_merchant_reason(&s.reason),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    entries: Vec<EntryView>,
    skipped: Vec<SkipView>,
    warnings: Vec<String>,
}

impl From<&Plan> for PlanView {
    fn from(plan: &Plan) -> Self {
        PlanView {
            entries: plan.entries.iter().map(EntryView::from).collect(),
            skipped: plan.skipped.iter().map(SkipView::from).collect(),
            warnings: plan.warnings.clone(),
        }
    }
}

/// Display details for one item of the plan: what the page shows beside the lister's fields.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub icon: Option<String>,
    pub rarity: &'static str,
    pub stash_label: String,
    pub rolls: Vec<String>,
}

/// What the game last showed of the player's listings.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingsInfo {
    pub seen: bool,
    pub free: Option<usize>,
    pub age_s: Option<u64>,
    pub payouts: usize,
    pub payout_gold: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResponse {
    pub plan: PlanView,
    pub items: HashMap<String, ItemInfo>,
    pub listings: ListingsInfo,
    pub needs_game_pricing: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(unique_id: &str, price: i64) -> EntryView {
        EntryView::from(&PlanEntry {
            unique_id: unique_id.into(),
            ..PlanEntry::new(unique_id, "Robe", 4, "2", 5, 2, 3, price, 999, 20)
        })
    }

    #[test]
    fn a_round_trip_keeps_the_entry_and_recomputes_the_fee() {
        let entry = view("7", 1000).into_entry(false).unwrap();
        assert_eq!(
            (
                entry.unique_id.as_str(),
                entry.price,
                entry.fee,
                entry.slot_id
            ),
            ("7", 1000, 50, 5)
        );
    }

    #[test]
    fn unpriced_entries_need_permission() {
        assert!(view("7", 0).into_entry(false).is_err());
        assert!(view("7", 0).into_entry(true).is_ok());
    }

    #[test]
    fn entries_are_limited_and_deduplicated() {
        assert!(parse_entries(Vec::new(), "list", false).is_err());
        let many = (0..=MAX_RUN_ITEMS)
            .map(|i| view(&i.to_string(), 100))
            .collect();
        assert!(parse_entries(many, "list", false).is_err());
        let twice = vec![view("1", 100), view("1", 100), view("2", 100)];
        assert_eq!(parse_entries(twice, "list", false).unwrap().len(), 2);
    }
}
