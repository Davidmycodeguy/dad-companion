//! Reads training listings straight from a `market_history.sqlite` file, without going through
//! `crate::history::MarketHistory` (training isn't on that type's list of things to support, and
//! this needs only a read-only connection over the same schema). Port of `MarketHistory
//! .worth_listings` (Python's `market_history.py`).

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::history::{DAY_S, LISTING_DAYS};

use super::TrainListing;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("market database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Every listing in the database at `path`, with `age_days` = how long it had been up when first
/// seen (`LISTING_DAYS - (expires_at - first_seen) / DAY_S`, clamped at 0).
pub fn load_listings(path: &Path) -> Result<Vec<TrainListing>, DbError> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = db.prepare("SELECT item_id, rarity, price, item_count, rolls, first_seen, expires_at FROM listings")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, f64>(5)?,
            row.get::<_, f64>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (item_id, rarity, price, item_count, rolls_json, first_seen, expires_at) = row?;
        let age_days = (LISTING_DAYS - (expires_at - first_seen) / DAY_S).max(0.0);
        out.push(TrainListing { item_id, rarity, price, item_count, rolls: parse_rolls(&rolls_json), age_days: Some(age_days) });
    }
    Ok(out)
}

/// `[["Luck", 17], ...]`; anything unreadable is no rolls rather than an error (matching
/// `crate::history`'s own tolerant `parse_stats`).
fn parse_rolls(json: &str) -> Vec<(String, f64)> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn seeded_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("market_history.sqlite");
        // Matches crate::history::MarketHistory's schema closely enough for this query.
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE listings (
                listing_id TEXT PRIMARY KEY, item_id TEXT NOT NULL, rarity INTEGER NOT NULL,
                price INTEGER NOT NULL, item_count INTEGER NOT NULL, base TEXT NOT NULL,
                rolls TEXT NOT NULL, seller TEXT NOT NULL, first_seen REAL NOT NULL,
                last_seen REAL NOT NULL, expires_at REAL NOT NULL, vanished_at REAL)",
        )
        .unwrap();
        let week = LISTING_DAYS * DAY_S;
        db.execute(
            "INSERT INTO listings VALUES ('a','Helm_5001',5,900,1,'[]','[[\"Luck\",17]]','',?1,?1,?2,NULL)",
            params![1_000_000.0, 1_000_000.0 + week],
        )
        .unwrap();
        // First seen 2 days into its week-long listing: age_days = 7 - (5/7 of a week) = 2.
        db.execute(
            "INSERT INTO listings VALUES ('b','Boots_5001',3,400,1,'[]','[]','',?1,?1,?2,NULL)",
            params![1_000_000.0, 1_000_000.0 + week - 2.0 * DAY_S],
        )
        .unwrap();
        (dir, path)
    }

    #[test]
    fn loads_every_listing_with_its_rolls_and_age() {
        let (_dir, path) = seeded_db();
        let mut listings = load_listings(&path).unwrap();
        listings.sort_by(|a, b| a.item_id.cmp(&b.item_id));
        assert_eq!(listings.len(), 2);
        assert_eq!(listings[0].item_id, "Boots_5001");
        assert!((listings[0].age_days.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(listings[1].item_id, "Helm_5001");
        assert_eq!(listings[1].rolls, vec![("Luck".to_string(), 17.0)]);
        assert!((listings[1].age_days.unwrap() - 0.0).abs() < 1e-9);
    }

    #[test]
    fn a_missing_database_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_listings(&dir.path().join("nope.sqlite")).is_err());
    }
}
