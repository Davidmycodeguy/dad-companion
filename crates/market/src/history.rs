//! Every Marketplace listing seen, in SQLite. The layout is DnDTools' `market_history.sqlite`, so
//! an imported database is used as is.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::Error;

pub const DAY_S: f64 = 86_400.0;
/// A Marketplace listing lasts a week.
pub const LISTING_DAYS: f64 = 7.0;
/// How long to wait for another connection (e.g. an analysis script) to finish writing.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS listings (
    listing_id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL,
    rarity INTEGER NOT NULL,
    price INTEGER NOT NULL,
    item_count INTEGER NOT NULL,
    base TEXT NOT NULL,
    rolls TEXT NOT NULL,
    seller TEXT NOT NULL,
    first_seen REAL NOT NULL,
    last_seen REAL NOT NULL,
    expires_at REAL NOT NULL,
    vanished_at REAL
);
CREATE INDEX IF NOT EXISTS idx_listings_item ON listings(item_id, last_seen);
CREATE TABLE IF NOT EXISTS scans (
    scan_id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id TEXT NOT NULL,
    started_at REAL NOT NULL,
    finished_at REAL NOT NULL,
    max_price INTEGER NOT NULL,
    complete INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS crawl_passes (
    rarity INTEGER NOT NULL,
    started_at REAL NOT NULL,
    finished_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS merchant_prices (
    item_id TEXT PRIMARY KEY,
    unit_price REAL NOT NULL,
    seen_at REAL NOT NULL
);
CREATE TABLE IF NOT EXISTS my_listings (
    listing_id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL,
    price INTEGER NOT NULL,
    state INTEGER NOT NULL,
    first_seen REAL NOT NULL,
    last_seen REAL NOT NULL,
    sold_at REAL
);
";

/// One stat of a listed item: a base stat or a random roll, as stored (percent stats ×10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub id: String,
    pub value: i64,
}

/// A listing as recorded. Times are Unix seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub listing_id: String,
    pub item_id: String,
    /// Price of the whole listing (all `count` items).
    pub price: i64,
    pub count: i64,
    pub base: Vec<Stat>,
    pub rolls: Vec<Stat>,
    pub first_seen: f64,
    pub last_seen: f64,
    pub expires_at: f64,
    /// When it disappeared before expiring (most likely sold).
    pub vanished_at: Option<f64>,
}

impl Listing {
    pub fn unit_price(&self) -> f64 {
        self.price as f64 / self.count.max(1) as f64
    }
}

/// The median ask among listings open on one day.
#[derive(Debug, Clone, PartialEq)]
pub struct DayMedian {
    pub day_start: f64,
    pub median: f64,
    pub listings: usize,
}

pub struct MarketHistory {
    db: Mutex<Connection>,
}

const LISTING_COLUMNS: &str =
    "listing_id, item_id, price, item_count, base, rolls, first_seen, last_seen, expires_at, vanished_at";

impl MarketHistory {
    /// Opens (or creates) the database at `path`.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let db = Connection::open(path)?;
        db.busy_timeout(BUSY_TIMEOUT)?;
        // WAL lets analysis scripts read while the app keeps recording pages.
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.execute_batch(SCHEMA)?;
        Ok(Self { db: Mutex::new(db) })
    }

    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, Error> {
        self.db.lock().map_err(|_| Error::Poisoned)
    }

    /// The item's listings still up for sale: seen within `max_age_s` of `now`, not vanished, not
    /// expired and not ours; cheapest per item first.
    pub fn open_listings(&self, item_id: &str, now: f64, max_age_s: f64) -> Result<Vec<Listing>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached(&format!(
            "SELECT {LISTING_COLUMNS} FROM listings
             WHERE item_id = ?1 AND vanished_at IS NULL AND last_seen >= ?2 AND expires_at > ?3
               AND listing_id NOT IN (SELECT listing_id FROM my_listings)"
        ))?;
        let mut listings = query
            .query_map(params![item_id, now - max_age_s, now], listing_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        listings.sort_by(|a, b| a.unit_price().total_cmp(&b.unit_price()));
        Ok(listings)
    }

    /// How many listings of each of `item_ids` are still up for sale (as in [`Self::open_listings`]);
    /// items without any are left out.
    pub fn open_listing_counts(&self, item_ids: &[&str], now: f64, max_age_s: f64) -> Result<HashMap<String, usize>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached(
            "SELECT COUNT(*) FROM listings
             WHERE item_id = ?1 AND vanished_at IS NULL AND last_seen >= ?2 AND expires_at > ?3
               AND listing_id NOT IN (SELECT listing_id FROM my_listings)",
        )?;
        let mut counts = HashMap::new();
        for id in item_ids {
            let count: i64 = query.query_row(params![id, now - max_age_s, now], |row| row.get(0))?;
            if count > 0 {
                counts.insert((*id).to_owned(), count as usize);
            }
        }
        Ok(counts)
    }

    /// Listings of the item that vanished before expiring (most likely sold) at or after `since`,
    /// newest first.
    pub fn probable_sales(&self, item_id: &str, since: f64) -> Result<Vec<Listing>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached(&format!(
            "SELECT {LISTING_COLUMNS} FROM listings
             WHERE item_id = ?1 AND last_seen >= ?2 AND vanished_at >= ?3
             ORDER BY vanished_at DESC"
        ))?;
        let sales = query
            .query_map(params![item_id, since - LISTING_DAYS * DAY_S, since], listing_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sales)
    }

    /// For each of the last `days` days (oldest first), the median ask among listings open that day:
    /// first seen by its end and last seen on or after its start. A first crawl holds the whole
    /// standing market, so counting only new listings per day would read as a price drop. Days with
    /// fewer than `min_listings` listings are left out.
    pub fn daily_medians(&self, item_id: &str, days: u32, now: f64, min_listings: usize) -> Result<Vec<DayMedian>, Error> {
        let window_start = now - f64::from(days) * DAY_S;
        let first_day = (window_start / DAY_S).floor() * DAY_S;
        let mut per_day: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
        {
            let db = self.db()?;
            let mut query = db.prepare_cached(
                "SELECT price, item_count, first_seen, last_seen FROM listings WHERE item_id = ?1 AND last_seen >= ?2",
            )?;
            let rows = query.query_map(params![item_id, window_start], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, f64>(2)?, row.get::<_, f64>(3)?))
            })?;
            for row in rows {
                let (price, count, first_seen, last_seen) = row?;
                let unit = price as f64 / count.max(1) as f64;
                let mut day = ((first_seen / DAY_S).floor() * DAY_S).max(first_day);
                // A damaged row seen "in the future" must not loop on for years.
                while day <= last_seen.min(now) {
                    per_day.entry(day as i64).or_default().push(unit);
                    day += DAY_S;
                }
            }
        }
        Ok(per_day
            .into_iter()
            .filter(|(_, prices)| prices.len() >= min_listings)
            .map(|(day, mut prices)| DayMedian { day_start: day as f64, median: median(&mut prices), listings: prices.len() })
            .collect())
    }

    /// Whether sales of the item since `since` could have been seen: one was, or since then a scan
    /// of the item or a full crawl of its rarity ran after its listings had been seen (those are
    /// what mark vanished listings; merely seeing listings again doesn't).
    pub fn sales_tracked(&self, item_id: &str, since: f64) -> Result<bool, Error> {
        let db = self.db()?;
        let window_start = since - LISTING_DAYS * DAY_S;
        let first: Option<f64> = db.query_row(
            "SELECT MIN(first_seen) FROM listings WHERE item_id = ?1 AND last_seen >= ?2",
            params![item_id, window_start],
            |row| row.get(0),
        )?;
        let Some(first) = first else { return Ok(false) };
        let found = db
            .query_row(
                "SELECT 1 WHERE EXISTS (SELECT 1 FROM listings WHERE item_id = ?1 AND last_seen >= ?2 AND vanished_at >= ?3)
                    OR EXISTS (SELECT 1 FROM scans WHERE item_id = ?1 AND finished_at >= ?3 AND started_at > ?4)
                    OR EXISTS (SELECT 1 FROM crawl_passes WHERE rarity = ?5 AND finished_at >= ?3 AND started_at > ?4)",
                params![item_id, window_start, since, first, rarity_of(item_id)],
                |_| Ok(()),
            )
            .optional()?;
        Ok(found.is_some())
    }
}

/// A listing as a marketplace page shows it (converted from the game's message by the caller).
#[derive(Debug, Clone, PartialEq)]
pub struct PageListing {
    pub listing_id: String,
    /// The catalog's id, e.g. "HeaterShield_5001" (see [`item_id_from_design`]).
    pub item_id: String,
    /// Price of the whole listing.
    pub price: i64,
    pub count: i64,
    pub base: Vec<Stat>,
    pub rolls: Vec<Stat>,
    pub seller: String,
    /// Time left before the listing expires.
    pub remain_ms: i64,
}

/// One of the player's own listings, from the My Listings pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyListing {
    pub listing_id: String,
    pub item_id: String,
    pub price: i64,
    /// The game's state: 1 listed, 3 sold.
    pub state: i32,
}

/// Something a merchant sells, as its shop shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerchantOffer {
    pub item_id: String,
    pub count: i64,
    /// Price for the whole `count`.
    pub final_price: i64,
}

/// Totals for the Overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub listings: u64,
    pub items: u64,
    pub vanished: u64,
    pub my_sold: u64,
}

/// How busy one item's market is: how many listings (or sales), and their unit prices.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemActivity {
    pub item_id: String,
    pub count: usize,
    pub lowest: f64,
    pub median: f64,
}

/// Groups `(item, unit price)` rows into activity per item, busiest first, at most `limit` items.
fn activity(rows: Vec<(String, f64)>, limit: usize) -> Vec<ItemActivity> {
    let mut per_item: HashMap<String, Vec<f64>> = HashMap::new();
    for (item_id, unit) in rows {
        per_item.entry(item_id).or_default().push(unit);
    }
    let mut items: Vec<ItemActivity> = per_item
        .into_iter()
        .map(|(item_id, mut units)| ItemActivity {
            count: units.len(),
            lowest: units.iter().copied().fold(f64::INFINITY, f64::min),
            median: median(&mut units),
            item_id,
        })
        .collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.item_id.cmp(&b.item_id)));
    items.truncate(limit);
    items
}

/// What [`MarketHistory::export_starter`] copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterSummary {
    pub listings: usize,
    pub merchant_prices: usize,
}

/// A listing gone more than this long before its expiry did not simply expire.
const VANISH_MARGIN_S: f64 = 600.0;
const MY_STATE_LISTING: i32 = 1;
const MY_STATE_SOLD: i32 = 3;
const MS_PER_S: f64 = 1000.0;
const ITEM_ID_PREFIX: &str = "Id_Item_";
const PROPERTY_PREFIX: &str = "Effect_";

/// "DesignDataItem:Id_Item_HeaterShield_5001" -> "HeaterShield_5001" (ids already bare pass through).
pub fn item_id_from_design(design_id: &str) -> String {
    design_id.rsplit(ITEM_ID_PREFIX).next().unwrap_or(design_id).to_owned()
}

/// "DesignDataItemPropertyType:Id_ItemPropertyType_Effect_Luck" -> "Luck".
pub fn stat_from_property(property_type_id: &str) -> String {
    property_type_id.rsplit(PROPERTY_PREFIX).next().unwrap_or(property_type_id).to_owned()
}

impl MarketHistory {
    /// Records the listings of a marketplace page seen at `now`: new ones are added, ones seen
    /// before get their price, last sighting and expiry updated (and are no longer vanished).
    pub fn record_listings(&self, rows: &[PageListing], now: f64) -> Result<usize, Error> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO listings (listing_id, item_id, rarity, price, item_count, base, rolls, seller,
                                       first_seen, last_seen, expires_at, vanished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, NULL)
                 ON CONFLICT(listing_id) DO UPDATE SET price=excluded.price, last_seen=excluded.last_seen,
                     expires_at=excluded.expires_at, vanished_at=NULL",
            )?;
            for row in rows {
                upsert.execute(params![
                    row.listing_id,
                    row.item_id,
                    rarity_of(&row.item_id),
                    row.price,
                    row.count.max(1),
                    stats_json(&row.base),
                    stats_json(&row.rolls),
                    row.seller,
                    now,
                    now + row.remain_ms as f64 / MS_PER_S,
                ])?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// Records a finished search of one item and marks listings that vanished since it started.
    /// Results come cheapest first, so an older listing priced below the highest price an
    /// incomplete scan reached (listings at exactly that price may have been cut off mid-page), or
    /// any at all if the scan read every page, that did not show up again and was not due to
    /// expire has most likely been sold. Returns how many were marked.
    pub fn note_scan(&self, item_id: &str, started_at: f64, max_price: i64, complete: bool, now: f64) -> Result<usize, Error> {
        let db = self.db()?;
        db.execute(
            "INSERT INTO scans (item_id, started_at, finished_at, max_price, complete) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![item_id, started_at, now, max_price, i64::from(complete)],
        )?;
        let marked = db.execute(
            "UPDATE listings SET vanished_at = ?1
             WHERE item_id = ?2 AND vanished_at IS NULL AND last_seen < ?3 AND expires_at > ?4
               AND (?5 = 1 OR price < ?6)",
            params![now, item_id, started_at, now + VANISH_MARGIN_S, i64::from(complete), max_price],
        )?;
        Ok(marked)
    }

    /// After a crawl read every page of one rarity, marks its listings that did not show up again
    /// (last seen before the pass started and not due to expire): most likely sold or cancelled.
    pub fn note_crawl_pass(&self, rarity: i64, started_at: f64, now: f64) -> Result<usize, Error> {
        let db = self.db()?;
        let marked = db.execute(
            "UPDATE listings SET vanished_at = ?1
             WHERE rarity = ?2 AND vanished_at IS NULL AND last_seen < ?3 AND expires_at > ?4",
            params![now, rarity, started_at, now + VANISH_MARGIN_S],
        )?;
        db.execute(
            "INSERT INTO crawl_passes (rarity, started_at, finished_at) VALUES (?1, ?2, ?3)",
            params![rarity, started_at, now],
        )?;
        Ok(marked)
    }

    /// Records the player's listings from a My Listings page; a sale keeps the first time it was seen.
    pub fn record_my_listings(&self, rows: &[MyListing], now: f64) -> Result<(), Error> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO my_listings (listing_id, item_id, price, state, first_seen, last_seen, sold_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
                 ON CONFLICT(listing_id) DO UPDATE SET state=excluded.state, last_seen=excluded.last_seen,
                     sold_at=COALESCE(my_listings.sold_at, excluded.sold_at)",
            )?;
            for row in rows {
                let sold_at = (row.state == MY_STATE_SOLD).then_some(now);
                upsert.execute(params![row.listing_id, row.item_id, row.price, row.state, now, sold_at])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Ids of the player's listings still up for sale, from every My Listings page ever seen.
    pub fn my_listing_ids(&self) -> Result<std::collections::HashSet<String>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached("SELECT listing_id FROM my_listings WHERE state = ?1")?;
        let ids = query.query_map(params![MY_STATE_LISTING], |row| row.get(0))?.collect::<Result<_, _>>()?;
        Ok(ids)
    }

    /// When the player's listing `listing_id` was first seen sold, if it has sold.
    pub fn my_sold_at(&self, listing_id: &str) -> Result<Option<f64>, Error> {
        let db = self.db()?;
        let sold = db
            .query_row("SELECT sold_at FROM my_listings WHERE listing_id = ?1", params![listing_id], |row| row.get(0))
            .optional()?;
        Ok(sold.flatten())
    }

    /// Records what a merchant sells and for how much per unit; the cheapest offer of each item is kept.
    pub fn record_merchant_stock(&self, offers: &[MerchantOffer], now: f64) -> Result<usize, Error> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let mut kept = 0;
        {
            let mut upsert = tx.prepare_cached(
                "INSERT INTO merchant_prices (item_id, unit_price, seen_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(item_id) DO UPDATE SET unit_price=MIN(unit_price, excluded.unit_price),
                     seen_at=excluded.seen_at",
            )?;
            for offer in offers.iter().filter(|o| !o.item_id.is_empty() && o.final_price > 0) {
                upsert.execute(params![offer.item_id, offer.final_price as f64 / offer.count.max(1) as f64, now])?;
                kept += 1;
            }
        }
        tx.commit()?;
        Ok(kept)
    }

    /// The cheapest price per unit a merchant was seen selling each item for.
    pub fn merchant_prices(&self) -> Result<HashMap<String, f64>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached("SELECT item_id, unit_price FROM merchant_prices")?;
        let prices = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(prices)
    }

    /// The cheapest price per unit a merchant sells `item_id` for, or None if none was seen selling it.
    pub fn merchant_price(&self, item_id: &str) -> Result<Option<f64>, Error> {
        let db = self.db()?;
        let price = db
            .query_row("SELECT unit_price FROM merchant_prices WHERE item_id = ?1", params![item_id], |row| row.get(0))
            .optional()?;
        Ok(price)
    }

    /// Writes a copy of the history seen since `since` to `dest`, safe to hand to other players:
    /// no seller names, none of our own listings, no record of what we sold. Whatever `dest` held
    /// is replaced; the copy is compacted and left as a single file.
    pub fn export_starter(&self, dest: &Path, since: f64) -> Result<StarterSummary, Error> {
        let fresh = MarketHistory::open(dest)?;
        fresh.db()?.execute_batch(
            "DELETE FROM listings; DELETE FROM scans; DELETE FROM crawl_passes; DELETE FROM merchant_prices; DELETE FROM my_listings;",
        )?;
        drop(fresh);
        let db = self.db()?;
        db.execute("ATTACH DATABASE ?1 AS starter", params![dest.to_string_lossy()])?;
        let copied = (|| -> Result<StarterSummary, Error> {
            let listings = db.execute(
                "INSERT INTO starter.listings
                 SELECT listing_id, item_id, rarity, price, item_count, base, rolls, '', first_seen, last_seen, expires_at, vanished_at
                 FROM main.listings
                 WHERE last_seen >= ?1 AND listing_id NOT IN (SELECT listing_id FROM main.my_listings)",
                params![since],
            )?;
            db.execute(
                "INSERT INTO starter.scans (item_id, started_at, finished_at, max_price, complete)
                 SELECT item_id, started_at, finished_at, max_price, complete FROM main.scans WHERE finished_at >= ?1",
                params![since],
            )?;
            db.execute(
                "INSERT INTO starter.crawl_passes SELECT rarity, started_at, finished_at FROM main.crawl_passes WHERE finished_at >= ?1",
                params![since],
            )?;
            let merchant_prices =
                db.execute("INSERT INTO starter.merchant_prices SELECT item_id, unit_price, seen_at FROM main.merchant_prices", [])?;
            Ok(StarterSummary { listings, merchant_prices })
        })();
        db.execute("DETACH DATABASE starter", [])?;
        let summary = copied?;
        let starter = Connection::open(dest)?;
        starter.pragma_update(None, "journal_mode", "DELETE")?;
        starter.execute_batch("VACUUM")?;
        Ok(summary)
    }

    /// The items with the most listings up for sale (seen within `max_age_s` of `now`, not ours),
    /// busiest first.
    pub fn most_listed(&self, now: f64, max_age_s: f64, limit: usize) -> Result<Vec<ItemActivity>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached(
            "SELECT item_id, price * 1.0 / MAX(item_count, 1) FROM listings
             WHERE vanished_at IS NULL AND last_seen >= ?1 AND expires_at > ?2
               AND listing_id NOT IN (SELECT listing_id FROM my_listings)",
        )?;
        let rows = query.query_map(params![now - max_age_s, now], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(activity(rows, limit))
    }

    /// The items with the most probable sales (listings gone before expiring) since `since`,
    /// busiest first, with the unit prices they sold at.
    pub fn fastest_selling(&self, since: f64, limit: usize) -> Result<Vec<ItemActivity>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached(
            "SELECT item_id, price * 1.0 / MAX(item_count, 1) FROM listings
             WHERE vanished_at >= ?1 AND listing_id NOT IN (SELECT listing_id FROM my_listings)",
        )?;
        let rows = query.query_map(params![since], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(activity(rows, limit))
    }

    /// Every saved listing, shaped for the cross-item pattern analysis (`crate::patterns::analyze`).
    pub fn pattern_listings(&self) -> Result<Vec<crate::patterns::Listing>, Error> {
        let db = self.db()?;
        let mut query = db.prepare_cached("SELECT item_id, rarity, price, item_count, base, rolls, seller FROM listings")?;
        let pairs = |json: String| serde_json::from_str::<Vec<(String, f64)>>(&json).unwrap_or_default();
        let listings = query
            .query_map([], |row| {
                Ok(crate::patterns::Listing {
                    item_id: row.get(0)?,
                    rarity: row.get(1)?,
                    price: row.get(2)?,
                    item_count: row.get(3)?,
                    base: pairs(row.get(4)?),
                    rolls: pairs(row.get(5)?),
                    seller: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(listings)
    }

    /// How many of `listing_ids` were already recorded before `before` (an incremental crawl stops
    /// once its pages are mostly listings it has seen).
    pub fn count_seen_before(&self, listing_ids: &[String], before: f64) -> Result<usize, Error> {
        if listing_ids.is_empty() {
            return Ok(0);
        }
        let db = self.db()?;
        let placeholders = vec!["?"; listing_ids.len()].join(",");
        let sql = format!("SELECT COUNT(*) FROM listings WHERE first_seen < ? AND listing_id IN ({placeholders})");
        let mut query = db.prepare(&sql)?;
        let values = std::iter::once(rusqlite::types::Value::Real(before))
            .chain(listing_ids.iter().map(|id| rusqlite::types::Value::Text(id.clone())));
        let count: i64 = query.query_row(rusqlite::params_from_iter(values), |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// When the newest saved listing was last seen (Unix seconds); None while the history is empty.
    pub fn newest_seen(&self) -> Result<Option<f64>, Error> {
        let db = self.db()?;
        Ok(db.query_row("SELECT MAX(last_seen) FROM listings", [], |row| row.get(0))?)
    }

    /// How much the history holds.
    pub fn summary(&self) -> Result<Summary, Error> {
        let db = self.db()?;
        let (listings, items, vanished): (i64, i64, Option<i64>) = db.query_row(
            "SELECT COUNT(*), COUNT(DISTINCT item_id), SUM(vanished_at IS NOT NULL) FROM listings",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let my_sold: i64 = db.query_row("SELECT COUNT(*) FROM my_listings WHERE state = ?1", params![MY_STATE_SOLD], |row| row.get(0))?;
        let count = |n: i64| u64::try_from(n).unwrap_or(0);
        Ok(Summary { listings: count(listings), items: count(items), vanished: count(vanished.unwrap_or(0)), my_sold: count(my_sold) })
    }
}

/// Stats as the history stores them: `[["Luck", 17], ...]`.
fn stats_json(stats: &[Stat]) -> String {
    serde_json::to_string(&stats.iter().map(|s| (s.id.as_str(), s.value)).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".into())
}

/// "HeaterShield_5001" -> 5; ids without a rarity suffix -> 0.
pub fn rarity_of(item_id: &str) -> i64 {
    match item_id.rsplit_once('_') {
        Some((_, suffix)) if suffix.len() == 4 && suffix.bytes().all(|b| b.is_ascii_digit()) => {
            i64::from(suffix.as_bytes()[0] - b'0')
        }
        _ => 0,
    }
}

fn listing_from_row(row: &Row<'_>) -> rusqlite::Result<Listing> {
    Ok(Listing {
        listing_id: row.get(0)?,
        item_id: row.get(1)?,
        price: row.get(2)?,
        count: row.get(3)?,
        base: parse_stats(&row.get::<_, String>(4)?),
        rolls: parse_stats(&row.get::<_, String>(5)?),
        first_seen: row.get(6)?,
        last_seen: row.get(7)?,
        expires_at: row.get(8)?,
        vanished_at: row.get(9)?,
    })
}

/// `[["ArmorPenetration", 30], ...]`; anything unreadable is no stats rather than an error.
fn parse_stats(json: &str) -> Vec<Stat> {
    serde_json::from_str::<Vec<(String, f64)>>(json)
        .map(|pairs| pairs.into_iter().map(|(id, value)| Stat { id, value: value.round() as i64 }).collect())
        .unwrap_or_default()
}

/// The middle value (the mean of the two middle ones for an even count); `values` gets sorted.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    match values.len() {
        0 => 0.0,
        n if n % 2 == 1 => values[mid],
        _ => (values[mid - 1] + values[mid]) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::{median, rarity_of};

    #[test]
    fn rarity_is_the_first_digit_of_a_four_digit_suffix() {
        assert_eq!(rarity_of("HeaterShield_5001"), 5);
        assert_eq!(rarity_of("GoldCoins"), 0);
        assert_eq!(rarity_of("Thing_501"), 0);
    }

    #[test]
    fn median_of_odd_and_even_counts() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&mut []), 0.0);
    }
}
