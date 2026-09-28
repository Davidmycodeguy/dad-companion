//! The last-second re-check before each item is listed: a fresh market search may lower an
//! approved price, or skip the item, but never raises it. Port of DnDTools' `_repricer`.

use std::collections::HashMap;

use tauri::{AppHandle, Manager};

use lister::job::{Reprice, RepriceDecision};
use lister::plan::{MarketBucket, PlanEntry, ABOVE_MAX_REASON, NOT_PRICED_REASON};
use market::{ListerRules, NO_SELLERS_REASON};

use crate::state::AppState;

/// A bigger fall than this is skipped for review, never listed far below what the player approved.
const MAX_RECHECK_DROP: f64 = 0.2;

pub struct Repricer {
    pub app: AppHandle,
    pub rules: ListerRules,
}

fn keep(price: i64, note: &str) -> RepriceDecision {
    RepriceDecision { price: Some(price), note: note.to_string() }
}

fn skip(note: String) -> RepriceDecision {
    RepriceDecision { price: None, note }
}

impl Reprice for Repricer {
    fn reprice(&self, entry: &PlanEntry, market: &MarketBucket) -> RepriceDecision {
        if entry.recommended != 0 && entry.price != entry.recommended {
            return keep(entry.price, "your price kept");
        }
        if market.degraded {
            return keep(entry.price, "market search incomplete, approved price kept");
        }
        let state = self.app.state::<AppState>();
        let unpriced = PlanEntry { price: 0, fee: 0, ..entry.clone() };
        let rows = HashMap::from([(entry.unique_id.clone(), market.clone())]);
        let plan = super::pricing_for(&state).game_prices(&[unpriced], &rows, &self.rules);
        decide(entry, &plan)
    }
}

/// What the fresh plan (for this one entry) means for the approved price.
fn decide(entry: &PlanEntry, plan: &lister::plan::Plan) -> RepriceDecision {
    let fresh = plan.entries.first();
    let skipped = plan.skipped.first();
    let (flag, confidence) = match (fresh, skipped) {
        (Some(e), _) => (e.flag.as_str(), e.confidence.as_str()),
        (None, Some(s)) => (s.flag.as_str(), s.confidence.as_str()),
        (None, None) => ("", ""),
    };
    if (fresh.is_some() || skipped.is_some()) && (!flag.is_empty() || confidence == "low") {
        return keep(entry.price, "fresh price uncertain, approved price kept");
    }
    let Some(fresh) = fresh else {
        let reason = skipped.map_or(NOT_PRICED_REASON, |s| s.reason.as_str());
        if reason == NO_SELLERS_REASON {
            return keep(entry.price, "no other sellers now, approved price kept");
        }
        if reason == ABOVE_MAX_REASON {
            return keep(entry.price, "market above the game's maximum, approved price kept");
        }
        return skip(format!("not worth listing at today's prices ({reason})"));
    };
    if (fresh.price as f64) < entry.price as f64 * (1.0 - MAX_RECHECK_DROP) {
        return skip(format!(
            "market dropped to {}g, more than {:.0}% below your {}g; price it again to review",
            fresh.price,
            MAX_RECHECK_DROP * 100.0,
            entry.price
        ));
    }
    RepriceDecision { price: Some(fresh.price.min(entry.price)), note: String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lister::plan::Plan;
    use market::Skip;

    fn entry(price: i64) -> PlanEntry {
        PlanEntry { recommended: price, ..PlanEntry::new("7", "Robe", 4, "2", 0, 2, 3, price, 0, 10) }
    }

    fn skipped(reason: &str) -> Skip {
        Skip { name: "Robe".into(), stash_id: "2".into(), slot_id: 0, reason: reason.into(), flag: String::new(), confidence: String::new(), unique_id: "7".into() }
    }

    #[test]
    fn a_lower_market_lowers_the_price_but_a_higher_one_never_raises_it() {
        let lower = Plan { entries: vec![PlanEntry { confidence: "high".into(), ..entry(900) }], ..Plan::default() };
        assert_eq!(decide(&entry(1000), &lower).price, Some(900));
        let higher = Plan { entries: vec![PlanEntry { confidence: "high".into(), ..entry(1500) }], ..Plan::default() };
        assert_eq!(decide(&entry(1000), &higher).price, Some(1000));
    }

    #[test]
    fn a_big_drop_skips_the_item_for_review() {
        let crash = Plan { entries: vec![PlanEntry { confidence: "high".into(), ..entry(700) }], ..Plan::default() };
        let decision = decide(&entry(1000), &crash);
        assert_eq!(decision.price, None);
        assert!(decision.note.contains("market dropped to 700g"), "{}", decision.note);
    }

    #[test]
    fn an_uncertain_fresh_price_keeps_the_approved_one() {
        let low = Plan { entries: vec![PlanEntry { confidence: "low".into(), ..entry(500) }], ..Plan::default() };
        assert_eq!(decide(&entry(1000), &low).price, Some(1000));
    }

    #[test]
    fn no_sellers_keeps_the_price_and_other_reasons_skip() {
        let none = Plan { skipped: vec![skipped(NO_SELLERS_REASON)], ..Plan::default() };
        assert_eq!(decide(&entry(1000), &none).price, Some(1000));
        let vendor = Plan { skipped: vec![skipped("vendor pays more")], ..Plan::default() };
        assert_eq!(decide(&entry(1000), &vendor).price, None);
    }
}
