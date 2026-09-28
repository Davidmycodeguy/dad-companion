//! The item page's market section: open listings, probable sales and daily prices of one item.

use serde::Serialize;
use tauri::State;

use market::{DayMedian, Listing, MarketHistory, Stat, DAY_S, LISTING_DAYS};

use crate::state::AppState;

/// Days of daily prices for the chart.
const HISTORY_DAYS: u32 = 30;
/// Sales are counted over the last week, the length of a listing.
const SALES_WINDOW_S: f64 = LISTING_DAYS * DAY_S;
/// Most listings sent to the page (a sanity cap: the busiest items have a few hundred open).
const MAX_LISTINGS: usize = 2_000;
/// Most ids counted in one call (an item has at most a handful of rarity variants).
const MAX_COUNTED_IDS: usize = 32;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatView {
    id: String,
    label: String,
    /// As the game shows it: "+1.7%", "+2".
    text: String,
}

impl From<&Stat> for StatView {
    fn from(stat: &Stat) -> Self {
        Self {
            id: stat.id.clone(),
            label: game_data::stat_label(&stat.id),
            text: game_data::roll_text(&stat.id, stat.value),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingView {
    id: String,
    price: i64,
    count: i64,
    unit_price: f64,
    rolls: Vec<StatView>,
    /// Seconds since it was last seen (for a sale: since it vanished).
    age_s: f64,
}

impl ListingView {
    fn new(listing: &Listing, since: f64, now: f64) -> Self {
        Self {
            id: listing.listing_id.clone(),
            price: listing.price,
            count: listing.count,
            unit_price: listing.unit_price(),
            rolls: listing.rolls.iter().map(StatView::from).collect(),
            age_s: (now - since).max(0.0),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayView {
    /// Start of the day, Unix seconds.
    day: f64,
    median: f64,
    listings: usize,
}

impl From<DayMedian> for DayView {
    fn from(day: DayMedian) -> Self {
        Self { day: day.day_start, median: day.median, listings: day.listings }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemMarketView {
    /// Open listings, cheapest per item first (at most `MAX_LISTINGS`).
    listings: Vec<ListingView>,
    listing_count: usize,
    lowest: Option<f64>,
    median: Option<f64>,
    /// Seconds since the newest open listing was seen.
    newest_age_s: Option<f64>,
    /// Probable sales in the last week, newest first.
    sales: Vec<ListingView>,
    /// Whether sales in the last week could have been seen at all (a crawl or scan ran).
    sales_tracked: bool,
    days: Vec<DayView>,
}

#[tauri::command]
pub fn item_market(state: State<'_, AppState>, item_id: String) -> Result<ItemMarketView, String> {
    let market = state.market.as_ref().ok_or_else(|| "The market history could not be opened.".to_owned())?;
    build(market, &item_id, now_s()).map_err(|err| err.to_string())
}

/// Open listings per item id (ids without any are left out), e.g. for an item's rarity variants.
#[tauri::command]
pub fn open_listing_counts(
    state: State<'_, AppState>,
    item_ids: Vec<String>,
) -> Result<std::collections::HashMap<String, usize>, String> {
    let market = state.market.as_ref().ok_or_else(|| "The market history could not be opened.".to_owned())?;
    let ids: Vec<&str> = item_ids.iter().map(String::as_str).take(MAX_COUNTED_IDS).collect();
    market.open_listing_counts(&ids, now_s(), LISTING_DAYS * DAY_S).map_err(|err| err.to_string())
}

fn now_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

fn build(market: &MarketHistory, item_id: &str, now: f64) -> Result<ItemMarketView, market::Error> {
    let open = market.open_listings(item_id, now, LISTING_DAYS * DAY_S)?;
    let since = now - SALES_WINDOW_S;
    let sales = market.probable_sales(item_id, since)?;
    let units: Vec<f64> = open.iter().map(Listing::unit_price).collect();
    Ok(ItemMarketView {
        listing_count: open.len(),
        lowest: units.first().copied(),
        median: middle(&units),
        newest_age_s: open.iter().map(|l| l.last_seen).reduce(f64::max).map(|seen| (now - seen).max(0.0)),
        listings: open.iter().take(MAX_LISTINGS).map(|l| ListingView::new(l, l.last_seen, now)).collect(),
        sales: sales.iter().take(MAX_LISTINGS).map(|l| ListingView::new(l, l.vanished_at.unwrap_or(l.last_seen), now)).collect(),
        sales_tracked: market.sales_tracked(item_id, since)?,
        days: market.daily_medians(item_id, HISTORY_DAYS, now, 1)?.into_iter().map(DayView::from).collect(),
    })
}

/// The median of values already sorted ascending.
fn middle(sorted: &[f64]) -> Option<f64> {
    let mid = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[mid]),
        _ => Some((sorted[mid - 1] + sorted[mid]) / 2.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_are_labelled_and_formatted_like_the_game() {
        let view = StatView::from(&Stat { id: "ActionSpeed".into(), value: 17 });
        assert_eq!((view.label.as_str(), view.text.as_str()), ("Action Speed", "+1.7%"));
    }

    #[test]
    fn middle_of_sorted_values() {
        assert_eq!(middle(&[]), None);
        assert_eq!(middle(&[1.0, 2.0, 9.0]), Some(2.0));
        assert_eq!(middle(&[1.0, 2.0, 4.0, 9.0]), Some(3.0));
    }

    #[test]
    fn a_new_history_has_no_prices() {
        let dir = std::env::temp_dir().join(format!("dad-market-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let market = MarketHistory::open(&dir.join("m.sqlite")).unwrap();

        let view = build(&market, "Sword_5001", 1_790_000_000.0).unwrap();

        assert_eq!((view.listing_count, view.lowest, view.sales_tracked), (0, None, false));
        drop(market);
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Listings seen within the last day count toward what is on the market.
const OVERVIEW_LISTED_WINDOW_S: f64 = DAY_S;
/// Items shown in each overview list.
const OVERVIEW_ITEMS: usize = 12;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    item_id: String,
    name: String,
    rarity: &'static str,
    icon: Option<String>,
    count: usize,
    lowest: f64,
    median: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketOverview {
    /// Items with the most listings up for sale.
    listed: Vec<ActivityView>,
    /// Items with the most probable sales this week.
    sold: Vec<ActivityView>,
    /// Seconds since the newest listing was seen; None without market data.
    newest_age_s: Option<f64>,
}

/// The market at a glance: what is listed most, and what sells fastest.
#[tauri::command]
pub fn market_overview(state: State<'_, AppState>) -> Result<MarketOverview, String> {
    let market = state.market.as_ref().ok_or_else(|| "The market history could not be opened.".to_owned())?;
    let now = now_s();
    let view = |activity: market::history::ItemActivity| {
        let item = state.catalog.get(&activity.item_id);
        ActivityView {
            name: item.map_or_else(|| activity.item_id.clone(), |i| i.name.clone()),
            rarity: item.map_or("Unknown", |i| i.rarity.name()),
            icon: item.and_then(|i| i.icon_path.clone()),
            item_id: activity.item_id,
            count: activity.count,
            lowest: activity.lowest,
            median: activity.median,
        }
    };
    let listed = market.most_listed(now, OVERVIEW_LISTED_WINDOW_S, OVERVIEW_ITEMS).map_err(|err| err.to_string())?;
    let sold = market.fastest_selling(now - SALES_WINDOW_S, OVERVIEW_ITEMS).map_err(|err| err.to_string())?;
    let newest = market.newest_seen().map_err(|err| err.to_string())?;
    Ok(MarketOverview {
        listed: listed.into_iter().map(view).collect(),
        sold: sold.into_iter().map(view).collect(),
        newest_age_s: newest.map(|seen| (now - seen).max(0.0)),
    })
}
