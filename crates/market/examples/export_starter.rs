//! Writes the starter market data the installer ships, so hover values and prices work before a
//! new player has browsed the Marketplace: no seller names and none of our own listings.
//!
//! cargo run --release -p market --example export_starter -- <market_history.sqlite> <out.sqlite> [days]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use market::MarketHistory;

/// Listings seen within this many days are kept by default.
const DEFAULT_DAYS: f64 = 14.0;
const DAY_S: f64 = 86_400.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: export_starter <market_history.sqlite> <out.sqlite> [days]";
    let source = PathBuf::from(args.next().ok_or(usage)?);
    let dest = PathBuf::from(args.next().ok_or(usage)?);
    let days: f64 = args.next().map(|d| d.parse()).transpose()?.unwrap_or(DEFAULT_DAYS);

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64();
    let history = MarketHistory::open(&source)?;
    let summary = history.export_starter(&dest, now - days * DAY_S)?;
    let size = std::fs::metadata(&dest)?.len();
    println!(
        "{} listings and {} merchant prices from the last {days} days -> {} ({:.1} MB)",
        summary.listings,
        summary.merchant_prices,
        dest.display(),
        size as f64 / 1_048_576.0
    );
    Ok(())
}
