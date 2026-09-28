//! The numbers on the hover value card: what an item is worth for its exact rolls, the price that
//! sells quickly, whether listing beats selling to a merchant, and what each roll adds. Port of
//! `build_card` in DnDTools' `hover_card.py`.

use serde::Serialize;

use crate::rules::listing_fee;
use crate::worth::{Confidence, Estimate};

/// A roll this close to the top of its range reads MAX.
const MAX_QUALITY: f64 = 0.98;
/// Fewer days of prices make a trend line noise.
const TREND_MIN_DAYS: usize = 4;
/// Too few listings to call the market fast or slow.
const PACE_MIN_LISTED: usize = 3;
/// Smaller pair effects are noise.
const SYNERGY_MIN_PCT: f64 = 3.0;
/// Weekly sales per listing at or above each edge, fastest first.
const PACES: [(f64, &str); 3] = [(0.25, "sells fast"), (0.08, "steady"), (0.0, "slow market")];

/// The tooltip as read: its title, rarity, random rolls (game-data units) and lines not understood.
#[derive(Debug, Clone, Default)]
pub struct TipFacts {
    pub title: String,
    pub rarity: String,
    pub rolls: Vec<(String, i64)>,
    pub unread: usize,
}

/// What the market history says about the item.
#[derive(Debug, Clone, Default)]
pub struct MarketFacts {
    /// Unit prices of other open listings.
    pub asks: Vec<f64>,
    /// Listings that vanished before expiring in the last 7 days.
    pub sold_week: u32,
    /// Daily median unit prices, oldest first.
    pub trend: Vec<f64>,
    /// How old the asks are when none were read lately.
    pub seen_ago_s: Option<f64>,
    /// Whether the market was read twice lately, so sales could have been seen.
    pub sales_tracked: bool,
    /// What the lister would list it at (roll pricing on live asks), when known.
    pub listing_price: Option<i64>,
}

/// Catalog facts about the item.
#[derive(Debug, Clone, Default)]
pub struct ItemFacts {
    /// The line under the title, e.g. "Chest · Plate".
    pub kind: String,
    pub icon: Option<String>,
    /// What a merchant pays for the whole stack.
    pub merchant: i64,
    /// Inventory cells it takes.
    pub slots: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// A fast sale nets more than a merchant pays.
    List,
    /// The merchant pays as much or more (or the fee eats the sale).
    Merchant,
    /// No market data to judge by.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollLine {
    pub label: String,
    /// As the game shows it, e.g. "+5.1%".
    pub value: String,
    /// 0..1 within the stat's range on this item.
    pub quality: Option<f64>,
    /// What the roll adds to the price against an average roll.
    pub gold: Option<i64>,
    pub is_max: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairLine {
    pub label: String,
    pub pct: f64,
}

/// Everything the card shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverCard {
    pub name: String,
    pub rarity: String,
    pub kind: String,
    pub icon: Option<String>,
    /// Typical ask for these exact rolls.
    pub value: Option<i64>,
    pub low: Option<i64>,
    pub high: Option<i64>,
    /// Lowest reasonable price: sells quickly.
    pub fast_sale: Option<i64>,
    /// The fast sale after the listing fee.
    pub net: Option<i64>,
    #[serde(serialize_with = "confidence_label")]
    pub confidence: Option<Confidence>,
    pub verdict: Verdict,
    /// Cheapest first.
    pub asks: Vec<i64>,
    pub seen_ago_s: Option<f64>,
    pub sold_week: u32,
    pub pace: Option<&'static str>,
    pub trend: Vec<f64>,
    pub trend_pct: Option<f64>,
    pub merchant: i64,
    pub slots: u32,
    pub per_slot: i64,
    pub rolls: Vec<RollLine>,
    /// 0..100 across the rolls.
    pub quality: Option<i64>,
    pub pairs: Vec<PairLine>,
    pub unread: usize,
    /// Listings the model learned this item from.
    pub listings: u64,
}

fn confidence_label<S: serde::Serializer>(value: &Option<Confidence>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(Confidence::High) => serializer.serialize_str("high"),
        Some(Confidence::Medium) => serializer.serialize_str("medium"),
        Some(Confidence::Low) => serializer.serialize_str("low"),
        Some(Confidence::Unknown) | None => serializer.serialize_none(),
    }
}

/// How quickly the item sells, from last week's sales against what is listed now.
pub fn market_pace(listed: usize, sold_week: u32) -> Option<&'static str> {
    if listed < PACE_MIN_LISTED {
        return None;
    }
    let ratio = f64::from(sold_week) / listed as f64;
    PACES.iter().find(|(edge, _)| ratio >= *edge).map(|(_, label)| *label)
}

/// Model effects are estimates: rounded so they don't look more exact than they are.
pub fn round_gold(value: f64) -> i64 {
    let step = if value.abs() >= 100.0 {
        10.0
    } else if value.abs() >= 20.0 {
        5.0
    } else {
        1.0
    };
    (step * round_half_even(value / step) as f64) as i64
}

/// Price change between the first and last two days, in percent, once there are enough days.
fn trend_pct(trend: &[f64]) -> Option<f64> {
    if trend.len() < TREND_MIN_DAYS {
        return None;
    }
    let start = (trend[0] + trend[1]) / 2.0;
    let end = (trend[trend.len() - 2] + trend[trend.len() - 1]) / 2.0;
    (start != 0.0).then(|| (end / start - 1.0) * 100.0)
}

/// Rounds like Python's `round()`: halves go to the even neighbour.
pub fn round_half_even(value: f64) -> i64 {
    let rounded = value.round();
    if (value - value.trunc()).abs() == 0.5 && rounded % 2.0 != 0.0 {
        (rounded - value.signum()) as i64
    } else {
        rounded as i64
    }
}

/// Everything the card shows, from the tooltip, its worth estimate (None without market data) and
/// what the market and the catalog say.
pub fn build_card(tip: &TipFacts, estimate: Option<&Estimate>, market: &MarketFacts, item: &ItemFacts) -> HoverCard {
    let typical = estimate.map_or(0.0, |e| e.typical);
    let rolls: Vec<RollLine> = tip
        .rolls
        .iter()
        .map(|(stat, value)| {
            let found = estimate.and_then(|e| e.rolls.iter().find(|r| &r.stat == stat));
            let quality = found.map(|r| r.quality);
            RollLine {
                label: game_data::stat_label(stat),
                value: game_data::roll_text(stat, *value),
                quality,
                gold: found.filter(|_| typical != 0.0).map(|r| round_gold(typical * r.effect_pct / 100.0)),
                is_max: quality.is_some_and(|q| q >= MAX_QUALITY),
            }
        })
        .collect();
    let known: Vec<f64> = rolls.iter().filter_map(|r| r.quality).collect();
    let value = estimate.map(|e| round_half_even(e.value));
    let fast_sale = estimate.map(|e| market.listing_price.unwrap_or_else(|| round_half_even(e.floor.min(e.value))));
    let net = fast_sale.map(|price| price - listing_fee(price));
    let merchant = item.merchant.max(0);
    let verdict = match net {
        None => Verdict::Unknown,
        // Listing only pays when a fast sale nets more than the merchant (and more than nothing).
        Some(net) if net <= 0 || (merchant > 0 && merchant >= net) => Verdict::Merchant,
        Some(_) => Verdict::List,
    };
    let slots = item.slots.max(1);
    let mut asks: Vec<i64> = market.asks.iter().map(|a| round_half_even(*a)).collect();
    asks.sort_unstable();
    HoverCard {
        name: tip.title.clone(),
        rarity: tip.rarity.clone(),
        kind: item.kind.clone(),
        icon: item.icon.clone(),
        value,
        low: estimate.map(|e| round_half_even(e.low)),
        high: estimate.map(|e| round_half_even(e.high)),
        fast_sale,
        net,
        confidence: estimate.map(|e| e.confidence),
        verdict,
        seen_ago_s: market.seen_ago_s,
        sold_week: market.sold_week,
        pace: if market.sales_tracked || market.sold_week > 0 { market_pace(asks.len(), market.sold_week) } else { None },
        asks,
        trend: market.trend.clone(),
        trend_pct: trend_pct(&market.trend),
        merchant,
        slots,
        per_slot: round_half_even(value.unwrap_or(merchant) as f64 / f64::from(slots)),
        quality: (!known.is_empty()).then(|| round_half_even(100.0 * known.iter().sum::<f64>() / known.len() as f64)),
        rolls,
        pairs: estimate
            .map(|e| {
                e.pairs
                    .iter()
                    .filter(|p| p.effect_pct.abs() >= SYNERGY_MIN_PCT)
                    .map(|p| PairLine {
                        label: p.stats.iter().map(|s| game_data::stat_label(s)).collect::<Vec<_>>().join(" + "),
                        pct: p.effect_pct,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        unread: tip.unread,
        listings: estimate.map_or(0, |e| e.listings),
    }
}

#[cfg(test)]
mod tests {
    use super::round_half_even;

    #[test]
    fn rounds_halves_to_even_like_python() {
        assert_eq!([round_half_even(0.5), round_half_even(1.5), round_half_even(2.5), round_half_even(-0.5), round_half_even(-1.5)], [0, 2, 2, 0, -2]);
        assert_eq!([round_half_even(2.4), round_half_even(2.6), round_half_even(-2.6)], [2, 3, -3]);
    }
}
