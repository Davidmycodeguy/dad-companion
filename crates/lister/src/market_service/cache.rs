//! A short-lived cache of successful price-check results, keyed by exact roll combination.
//!
//! Port of `market_service.py`'s module-level `_market_price_cache`. That cache is a process-wide
//! global (a Python module is a singleton); an instantiable struct is used here instead so callers
//! choose their own lifetime for it (one per app, or a fresh one per test) rather than fighting over
//! shared global state the way the Python tests must (each starts with `clear_market_cache()`).

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use super::{MARKET_CACHE_DURATION_S, MARKET_CACHE_MAX_ENTRIES};

struct Entry {
    timestamp: f64,
    data: Value,
}

/// Caches [`super::normalize_v2_price_check_response`]-shaped results for
/// [`super::fetch_price_check`], bounded in both age and count.
pub struct MarketPriceCache {
    entries: Mutex<HashMap<String, Entry>>,
    duration_s: f64,
    max_entries: usize,
}

impl Default for MarketPriceCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketPriceCache {
    pub fn new() -> Self {
        Self::with_limits(MARKET_CACHE_DURATION_S, MARKET_CACHE_MAX_ENTRIES)
    }

    pub fn with_limits(duration_s: f64, max_entries: usize) -> Self {
        MarketPriceCache { entries: Mutex::new(HashMap::new()), duration_s, max_entries }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().expect("MarketPriceCache mutex is never held across a panic")
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Evicts every entry older than the cache duration, then returns a clone of `cache_key`'s data
    /// if it is still (now) present.
    pub fn get(&self, cache_key: &str, now: f64) -> Option<Value> {
        let mut entries = self.lock();
        entries.retain(|_, entry| now - entry.timestamp < self.duration_s);
        entries.get(cache_key).map(|entry| entry.data.clone())
    }

    /// Stores `result` under `cache_key`, then evicts the single oldest entry repeatedly until at
    /// most `max_entries` remain.
    pub fn insert(&self, cache_key: String, result: Value, now: f64) {
        let mut entries = self.lock();
        entries.insert(cache_key, Entry { timestamp: now, data: result });
        while entries.len() > self.max_entries {
            let Some(oldest) = entries.iter().min_by(|a, b| a.1.timestamp.total_cmp(&b.1.timestamp)).map(|(k, _)| k.clone()) else { break };
            entries.remove(&oldest);
        }
    }
}
