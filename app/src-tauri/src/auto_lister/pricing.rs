//! Pricing a plan without the game: from the listings the app has saved, or from the value formula.
//! Also the pricer a game search feeds, shared by "Price from game" and the re-check before listing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lister::plan::{
    apply_game_prices, price_from_model, MarketBucket, ModelPrice, Plan, PlanEntry,
};
use market::{Estimate, ListerRules, MarketHistory, MarketRow, WorthModel};

/// Listings seen in the last 6 hours count as the live market.
pub const HISTORY_MAX_AGE_S: f64 = 6.0 * 3600.0;
/// Plans priced without the game are capped after pricing, so unpriceable items don't use up spots.
const UNCAPPED_ITEMS: i64 = 10_000;

/// What pricing reads: saved listings, the value model, our own listings (never competition).
pub struct Pricing<'a> {
    pub market: Option<&'a MarketHistory>,
    pub worth: Option<Arc<WorthModel>>,
    /// Stat pairs that sell for more together, and the share of extra-roll premiums buyers pay.
    pub learned: Arc<crate::training::Learned>,
    pub own_listing_ids: HashSet<String>,
    pub now: f64,
}

impl Pricing<'_> {
    /// Recent saved listings of an item (our own left out by the history itself).
    fn history_rows(&self, item_id: &str) -> Vec<MarketRow> {
        let Some(market) = self.market else {
            return Vec::new();
        };
        match market.open_listings(item_id, self.now, HISTORY_MAX_AGE_S) {
            Ok(listings) => listings.iter().map(MarketRow::from).collect(),
            Err(err) => {
                log::warn!("saved listings of {item_id} could not be read: {err}");
                Vec::new()
            }
        }
    }

    /// The value model's estimate for an entry's exact rolls, only for items it learned.
    fn estimate(&self, entry: &PlanEntry) -> Option<Estimate> {
        let model = self.worth.as_deref()?;
        if entry.item_id.is_empty() || !model.knows(&entry.item_id) {
            return None;
        }
        let rolls: Vec<(String, f64)> = entry
            .rolls
            .iter()
            .map(|(stat, value)| (stat.clone(), *value as f64))
            .collect();
        let quantity = u32::try_from(entry.quantity.max(1)).unwrap_or(1);
        Some(model.predict(&entry.item_id, &rolls, quantity, None, None))
    }

    fn merchant_price(&self, item_id: &str) -> Option<f64> {
        self.market
            .and_then(|m| m.merchant_price(item_id).ok().flatten())
    }

    /// Prices entries from market search results (by unique id), widened with saved listings.
    pub fn game_prices(
        &self,
        entries: &[PlanEntry],
        rows: &HashMap<String, MarketBucket>,
        rules: &ListerRules,
    ) -> Plan {
        let extra_rows = |item_id: &str| self.history_rows(item_id);
        let worth = |entry: &PlanEntry| self.estimate(entry).map(ModelPrice::Estimate);
        let merchant = |item_id: &str| self.merchant_price(item_id);
        apply_game_prices(
            entries,
            rows,
            rules,
            Some(&extra_rows),
            self.learned.extra_share,
            &self.own_listing_ids,
            Some(&self.learned.synergies),
            Some(&worth),
            Some(&merchant),
        )
    }

    /// Prices an unpriced plan from saved listings ("database") or the value formula ("model"), then
    /// keeps as many priced items as the run may list.
    pub fn price_without_game(
        &self,
        rules: &ListerRules,
        unpriced: Plan,
        free_spots: Option<i64>,
    ) -> Plan {
        let priced = if rules.price_source == "model" {
            let worth = |entry: &PlanEntry| self.estimate(entry);
            price_from_model(&unpriced.entries, rules, Some(&worth))
        } else {
            // The saved listings of each item stand in for a live search.
            let empty: HashMap<String, MarketBucket> = unpriced
                .entries
                .iter()
                .map(|e| (e.unique_id.clone(), MarketBucket::default()))
                .collect();
            self.game_prices(&unpriced.entries, &empty, rules)
        };
        let limit = free_spots.map_or(rules.max_items_per_run, |spots| {
            rules.max_items_per_run.min(spots.max(0))
        });
        let limit = usize::try_from(limit).unwrap_or(0);
        let mut warnings: Vec<String> = unpriced
            .warnings
            .into_iter()
            .chain(priced.warnings)
            .filter(|w| !w.contains("Price from game"))
            .collect();
        if priced.entries.len() > limit {
            warnings.push(format!("Only {limit} can be listed now (free spots / max per run), the rest were left out."));
        }
        let mut skipped = unpriced.skipped;
        skipped.extend(priced.skipped);
        Plan {
            entries: priced.entries.into_iter().take(limit).collect(),
            skipped,
            warnings,
        }
    }
}

/// The rules with the per-run cap lifted, for plans priced after they are built.
pub fn uncapped(rules: &ListerRules) -> ListerRules {
    ListerRules {
        max_items_per_run: UNCAPPED_ITEMS,
        ..rules.clone()
    }
}
