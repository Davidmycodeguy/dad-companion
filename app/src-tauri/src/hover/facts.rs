//! Gathers what the hover card shows about one item: the price model's estimate for its exact
//! rolls, the market history's asks, sales and trend, and the catalog's merchant price and size.
//! The windows match the Python app's hover values.

use market::card::{build_card, HoverCard, ItemFacts, MarketFacts, TipFacts};
use market::{Confidence, MarketHistory, DAY_S};

use crate::state::AppState;

/// Listings seen this recently count as the live market; older asks carry their age.
const LIVE_S: f64 = 6.0 * 3600.0;
/// Asks, sales and sales tracking look back a week, the length of a listing.
const WEEK_S: f64 = 7.0 * DAY_S;
/// Days of daily prices behind the trend.
const TREND_DAYS: u32 = 14;
/// A day with fewer open listings says little about the price level.
const TREND_MIN_LISTINGS: usize = 5;

pub fn now_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

/// The card for `item_id` with the rolls read from its tooltip.
pub fn card_for(state: &AppState, item_id: &str, tip: &TipFacts, now: f64) -> HoverCard {
    let rolls: Vec<(String, f64)> = tip.rolls.iter().map(|(stat, value)| (stat.clone(), *value as f64)).collect();
    let estimate = state
        .worth()
        .map(|model| model.predict(item_id, &rolls, 1, None, None))
        .filter(|estimate| estimate.confidence != Confidence::Unknown);
    let market = state.market.as_ref().map(|history| market_facts(history, item_id, now)).unwrap_or_default();
    let item = state
        .catalog
        .get(item_id)
        .map(|item| ItemFacts {
            kind: item.kind_line(),
            icon: item.icon_path.clone(),
            merchant: i64::from(item.vendor_price),
            slots: item.slots(),
        })
        .unwrap_or_default();
    build_card(tip, estimate.as_ref(), &market, &item)
}

/// Open asks (with their age when none were seen lately), last week's sales and the daily trend.
fn market_facts(history: &MarketHistory, item_id: &str, now: f64) -> MarketFacts {
    let open = history.open_listings(item_id, now, WEEK_S).unwrap_or_default();
    let newest = open.iter().map(|listing| listing.last_seen).reduce(f64::max);
    let since = now - WEEK_S;
    MarketFacts {
        asks: open.iter().map(|listing| listing.unit_price()).collect(),
        seen_ago_s: newest.filter(|seen| *seen < now - LIVE_S).map(|seen| now - seen),
        sold_week: history.probable_sales(item_id, since).map_or(0, |sales| sales.len() as u32),
        sales_tracked: history.sales_tracked(item_id, since).unwrap_or(false),
        trend: history
            .daily_medians(item_id, TREND_DAYS, now, TREND_MIN_LISTINGS)
            .map(|days| days.into_iter().map(|day| day.median).collect())
            .unwrap_or_default(),
        listing_price: None,
    }
}
