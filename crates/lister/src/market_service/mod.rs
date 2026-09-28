//! DarkerDB market price lookups: request/response shaping, normalization and caching.
//!
//! Port of the logic in DnDTools' `src/market_service.py` (not the Flask endpoints it backs). The
//! actual HTTP call is behind [`HttpGet`] so this crate does not need to depend on an HTTP client;
//! a later phase supplies a real one (reqwest or similar).
//!
//! Results are shaped as [`serde_json::Value`] objects rather than dedicated structs: the exact
//! field set differs by endpoint and by success/error path (this mirrors the Python dicts closely,
//! e.g. `has_data`, `avg_price`, `confidence`, `rate_limit`), and it is exactly the JSON contract
//! callers on the frontend already expect.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::plan::{json_str, json_truthy};

mod cache;
mod client;

pub use cache::MarketPriceCache;
pub use client::{fetch_market_listing_estimate, fetch_market_listings, fetch_price_check, HttpError, HttpGet, HttpResponse};

pub const DARKERDB_BASE_URL: &str = "https://api.darkerdb.com";
pub const MARKET_LISTINGS_PATH: &str = "/v2/market";
pub const PRICE_CHECK_PATH: &str = "/v2/price-checks";
pub const DARKERDB_API_VERSION: &str = "2026-08-03";
pub const MARKET_CACHE_DURATION_S: f64 = 300.0;
pub const MARKET_CACHE_MAX_ENTRIES: usize = 500;
pub const USER_AGENT: &str = "DnDTools-MarketProxy/2.0";
/// Env var names checked, in order, for the DarkerDB API key.
pub const API_KEY_ENV_VARS: [&str; 2] = ["DARKERDB_API_KEY", "DNDTOOLS_DARKERDB_API_KEY"];

fn rarity_normalize_map() -> &'static [(&'static str, &'static str)] {
    &[
        ("poor", "Poor"),
        ("common", "Common"),
        ("uncommon", "Uncommon"),
        ("rare", "Rare"),
        ("epic", "Epic"),
        ("legend", "Legendary"),
        ("legendary", "Legendary"),
        ("unique", "Unique"),
        ("mythic", "Mythic"),
        ("artifact", "Artifact"),
    ]
}

/// Canonical display rarity for a lowercase/aliased input (`"legend"` and `"legendary"` both become
/// `"Legendary"`); unrecognised non-empty input passes through unchanged, matching Python's
/// `_RARITY_NORMALIZE.get(key, rarity)` fallback.
pub fn normalize_rarity(rarity: &str) -> String {
    if rarity.is_empty() {
        return String::new();
    }
    let key = rarity.to_lowercase();
    rarity_normalize_map().iter().find(|(k, _)| *k == key).map_or_else(|| rarity.to_string(), |(_, v)| v.to_string())
}

/// camelCase/space/hyphen -> `snake_case`, ASCII alphanumeric and `_` only, without trimming
/// leading/trailing underscores. Shared core of `_attribute_key` and `_camel_to_snake`, which
/// differ only in that final trim.
fn snake_case_core(value: &str) -> String {
    let mut underscored = String::with_capacity(value.len() + 4);
    let mut prev: Option<char> = None;
    for ch in value.trim().chars() {
        if ch.is_ascii_uppercase() && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
            underscored.push('_');
        }
        underscored.push(ch);
        prev = Some(ch);
    }
    let mut collapsed = String::with_capacity(underscored.len());
    let mut last_was_sep = false;
    for ch in underscored.chars() {
        if ch.is_whitespace() || ch == '-' {
            if !last_was_sep {
                collapsed.push('_');
            }
            last_was_sep = true;
        } else {
            collapsed.push(ch);
            last_was_sep = false;
        }
    }
    collapsed.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<String>().to_lowercase()
}

/// A roll/attribute name as a DarkerDB query key, e.g. `"Physical Power"` -> `"physical_power"`.
fn attribute_key(name: &str) -> String {
    snake_case_core(name)
}

/// An item id / archetype fragment as DarkerDB's dotted id scheme expects it, with no leading or
/// trailing `_` (`_camel_to_snake` in the Python).
fn camel_to_snake(value: &str) -> String {
    snake_case_core(value).trim_matches('_').to_string()
}

/// The DarkerDB API key `get_env` reports, checking [`API_KEY_ENV_VARS`] in order and trimming
/// whitespace; `""` if none is set. Takes the lookup as a parameter — rather than reading
/// `std::env::var` directly — so callers, and this crate's own tests, never have to mutate the
/// real process environment (which races across parallel test threads).
pub fn darkerdb_api_key(get_env: impl Fn(&str) -> Option<String>) -> String {
    for name in API_KEY_ENV_VARS {
        if let Some(value) = get_env(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    String::new()
}

/// [`darkerdb_api_key`] reading the real process environment, for production wiring.
pub fn darkerdb_api_key_from_env() -> String {
    darkerdb_api_key(|name| std::env::var(name).ok())
}

/// `User-Agent` / `X-API-Version` (and `X-Api-Key` when `api_key` is non-empty) for a DarkerDB
/// request.
pub fn darkerdb_headers(api_key: &str) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".to_string(), USER_AGENT.to_string());
    headers.insert("X-API-Version".to_string(), DARKERDB_API_VERSION.to_string());
    if !api_key.is_empty() {
        headers.insert("X-Api-Key".to_string(), api_key.to_string());
    }
    headers
}

/// A canonical, order-independent cache key for one exact roll combination.
pub fn build_cache_key(item_name: &str, rarity: &str, pp: &[(String, Value)], sp: &[(String, Value)], item_id: &str, archetype: &str) -> String {
    let mut parts = vec![item_name.to_string(), normalize_rarity(rarity)];
    let identity = item_id.trim();
    if !identity.is_empty() {
        parts.push(format!("id:{}", identity.to_lowercase()));
    } else {
        let family = archetype.trim();
        if !family.is_empty() {
            parts.push(format!("archetype:{}", family.to_lowercase()));
        }
    }
    for (prefix, props) in [("p", pp), ("s", sp)] {
        let mut sorted: Vec<&(String, Value)> = props.iter().collect();
        sorted.sort_by_key(|(name, _)| attribute_key(name));
        for (name, value) in sorted {
            parts.push(format!("{prefix}:{}={}", attribute_key(name), json_str(value)));
        }
    }
    parts.join("|")
}

/// `item_id` as DarkerDB's dotted id scheme (`"FrostAmulet_6001"` -> `"id.item.frost_amulet_6001"`);
/// already-dotted ids pass through, and a blank id stays blank.
pub fn to_darkerdb_item_id(item_id: &str) -> String {
    let text = item_id.trim();
    if text.is_empty() {
        String::new()
    } else if text.starts_with("id.item.") {
        text.to_string()
    } else {
        format!("id.item.{}", camel_to_snake(text))
    }
}

/// `archetype` (or, failing that, the family prefix of `item_id` up to its first `_`) as DarkerDB's
/// dotted id scheme; blank when neither yields anything.
pub fn to_darkerdb_archetype(item_id: &str, archetype: &str) -> String {
    let text = archetype.trim();
    if text.starts_with("id.item.") {
        return text.to_string();
    }
    let text = if text.is_empty() { item_id.split('_').next().unwrap_or("") } else { text };
    if text.is_empty() {
        String::new()
    } else {
        format!("id.item.{}", camel_to_snake(text))
    }
}

/// Raised when a caller-supplied roll list (`pp`/`sp`) is malformed or unbounded. Ported from the
/// `ValueError`s `normalize_market_properties` raises.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidMarketProperties(pub &'static str);

/// Validates a raw `[[name, value], ...]` roll list before it reaches the network: at most
/// `max_count` entries, each a 2-element array of a non-blank name (<=128 chars) and a scalar value
/// (a string, number or bool — a string value is additionally capped at 128 chars).
pub fn normalize_market_properties(value: Option<&Value>, max_count: usize) -> Result<Vec<(String, Value)>, InvalidMarketProperties> {
    let Some(value) = value else { return Ok(Vec::new()) };
    let Value::Array(entries) = value else {
        return Err(InvalidMarketProperties("Market properties must be a bounded array"));
    };
    if entries.len() > max_count {
        return Err(InvalidMarketProperties("Market properties must be a bounded array"));
    }
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        let Value::Array(pair) = entry else {
            return Err(InvalidMarketProperties("Each market property must contain a name and value"));
        };
        if pair.len() < 2 {
            return Err(InvalidMarketProperties("Each market property must contain a name and value"));
        }
        let name = json_str(&pair[0]).trim().to_string();
        let prop_value = pair[1].clone();
        let scalar = matches!(prop_value, Value::String(_) | Value::Number(_) | Value::Bool(_));
        if name.is_empty() || name.chars().count() > 128 || !scalar {
            return Err(InvalidMarketProperties("Market property contains an invalid name or value"));
        }
        if matches!(&prop_value, Value::String(s) if s.chars().count() > 128) {
            return Err(InvalidMarketProperties("Market property value is too long"));
        }
        normalized.push((name, prop_value));
    }
    Ok(normalized)
}

/// Query params for the legacy (v1) price-check request.
pub fn build_price_check_params(item_name: &str, rarity: &str, pp: &[(String, Value)], sp: &[(String, Value)]) -> Vec<(String, String)> {
    let mut params = vec![("item".to_string(), item_name.to_string())];
    let normalized_rarity = normalize_rarity(rarity);
    if !normalized_rarity.is_empty() {
        params.push(("rarity".to_string(), normalized_rarity));
    }
    for (name, value) in pp {
        params.push((format!("primary[{}]", attribute_key(name)), json_str(value)));
    }
    for (name, value) in sp {
        params.push((format!("secondary[{}]", attribute_key(name)), json_str(value)));
    }
    params
}

/// Query params for the current (v2) exact-price-check request, without leaking credentials.
pub fn build_v2_price_check_params(item_id: &str, pp: &[(String, Value)], sp: &[(String, Value)]) -> Vec<(String, String)> {
    let mut params = vec![("item_id".to_string(), to_darkerdb_item_id(item_id))];
    for (name, value) in pp.iter().chain(sp) {
        if matches!(value, Value::Null) || matches!(value, Value::String(s) if s.is_empty()) {
            continue;
        }
        params.push((format!("attributes[{}]", attribute_key(name)), json_str(value)));
    }
    params
}

// --- timestamps, freshness and confidence --------------------------------------------------

/// Python `datetime`'s representable range (year 1 to year 9999), as Unix seconds; a numeric
/// timestamp outside it is what `_parse_timestamp` catches `OverflowError` for.
const MIN_TIMESTAMP: f64 = -62_135_596_800.0;
const MAX_TIMESTAMP: f64 = 253_402_300_799.0;

/// Days since 1970-01-01 for a proleptic-Gregorian civil date (Howard Hinnant's `days_from_civil`
/// algorithm), valid for any year Rust's `i64` can hold — used instead of a date/time library
/// dependency for the one conversion this crate needs.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The inverse of [`days_from_civil`]: days since 1970-01-01 -> `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// `fetched_at` (Unix seconds) as `datetime.fromtimestamp(fetched_at, tz=utc).isoformat()` would
/// print it: no fractional part when whole, else exactly 6 fractional digits.
fn iso8601_utc(epoch_seconds: f64) -> String {
    let mut whole_seconds = epoch_seconds.floor();
    let mut micros = ((epoch_seconds - whole_seconds) * 1_000_000.0).round() as i64;
    if micros >= 1_000_000 {
        micros -= 1_000_000;
        whole_seconds += 1.0;
    }
    let days = (whole_seconds / 86_400.0).floor();
    let secs_of_day = (whole_seconds - days * 86_400.0) as i64;
    let (y, m, d) = civil_from_days(days as i64);
    let (hour, minute, second) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    if micros == 0 {
        format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}+00:00")
    } else {
        format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}+00:00")
    }
}

fn epoch_seconds(y: i64, m: u32, d: u32, hour: u32, minute: u32, second: f64) -> Option<f64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hour > 23 || minute > 59 || !(0.0..60.0).contains(&second) {
        return None;
    }
    let days = days_from_civil(y, m as i64, d as i64);
    Some(days as f64 * 86_400.0 + hour as f64 * 3600.0 + minute as f64 * 60.0 + second)
}

/// `"+HH:MM"` / `"-HH:MM"` -> signed offset minutes; `""` (naive) -> `0` (treated as UTC).
fn parse_offset_minutes(s: &str) -> Option<i64> {
    if s.is_empty() {
        return Some(0);
    }
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (hh, mm) = s[1..].split_once(':').unwrap_or((&s[1..], "0"));
    Some(sign * (hh.parse::<i64>().ok()? * 60 + mm.parse::<i64>().ok()?))
}

/// `"YYYY-MM-DDTHH:MM:SS[.ffffff][Z|+HH:MM|-HH:MM]"` -> Unix seconds. Naive (no offset) is UTC,
/// matching `_parse_timestamp` giving a naive result `tzinfo=timezone.utc`.
fn parse_iso_datetime(text: &str) -> Option<f64> {
    let normalized = text.replace('Z', "+00:00");
    let (date_part, time_part) = normalized.split_once('T')?;
    let (y, m, d) = parse_date(date_part)?;
    let offset_at = time_part.rfind(['+', '-']);
    let (time_only, offset_str) = offset_at.map_or((time_part, ""), |i| (&time_part[..i], &time_part[i..]));
    let offset_minutes = parse_offset_minutes(offset_str)?;
    let mut parts = time_only.splitn(3, ':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    let second: f64 = parts.next()?.parse().ok()?;
    Some(epoch_seconds(y, m, d, hour, minute, second)? - offset_minutes as f64 * 60.0)
}

fn parse_date(s: &str) -> Option<(i64, u32, u32)> {
    let mut parts = s.splitn(3, '-');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// The `"%Y-%m-%d %H:%M:%S"` fallback format, always naive/UTC.
fn parse_space_datetime(text: &str) -> Option<f64> {
    let (date_part, time_part) = text.split_once(' ')?;
    let (y, m, d) = parse_date(date_part)?;
    let mut parts = time_part.splitn(3, ':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    let second: f64 = parts.next()?.parse().ok()?;
    epoch_seconds(y, m, d, hour, minute, second)
}

/// A JSON timestamp (epoch number or an ISO-ish string) as Unix seconds, or `None` if it is falsy,
/// out of `datetime`'s representable range, or not a format DarkerDB actually sends.
fn parse_timestamp(value: &Value) -> Option<f64> {
    if !json_truthy(value) {
        return None;
    }
    match value {
        Value::Number(n) => n.as_f64().filter(|f| (MIN_TIMESTAMP..=MAX_TIMESTAMP).contains(f)),
        Value::String(s) => {
            let text = s.trim();
            (!text.is_empty()).then(|| parse_iso_datetime(text).or_else(|| parse_space_datetime(text))).flatten()
        }
        _ => None,
    }
}

fn freshness(updated_at: &Value, fetched_at: f64) -> &'static str {
    let Some(observed) = parse_timestamp(updated_at) else { return "unknown" };
    let age_seconds = (fetched_at - observed).max(0.0);
    if age_seconds <= 900.0 {
        "fresh"
    } else if age_seconds <= 86_400.0 {
        "recent"
    } else {
        "stale"
    }
}

fn confidence_level(sample_count: i64) -> &'static str {
    if sample_count >= 12 {
        "high"
    } else if sample_count >= 4 {
        "medium"
    } else if sample_count >= 1 {
        "low"
    } else {
        "none"
    }
}

/// The rate-limit headers DarkerDB reports, carried through into the result dict untouched.
pub fn rate_limit_from_headers(headers: &HashMap<String, String>) -> Value {
    let get = |k: &str| headers.get(k).cloned().map_or(Value::Null, Value::String);
    json!({
        "limit": get("X-RateLimit-Limit"),
        "remaining": get("X-RateLimit-Remaining"),
        "reset": get("X-RateLimit-Reset"),
        "retry_after": get("Retry-After"),
    })
}

fn number_as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        _ => None,
    }
}

/// Python `float(x)`'s tolerance for numbers, numeric strings and bools; anything else (or an
/// unparseable string) fails, matching the `except (TypeError, ValueError)` DnDTools guards with.
fn as_f64_loose(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn median(values: &[f64]) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if ordered.is_empty() {
        return 0.0;
    }
    let mid = ordered.len() / 2;
    if ordered.len() % 2 == 1 {
        ordered[mid]
    } else {
        (ordered[mid - 1] + ordered[mid]) / 2.0
    }
}

/// Drops listings priced far above the pack (likely a mistake, not a real ask) when there is
/// enough data to tell: at least 4 rows, a positive median, and the cut leaving at least half the
/// rows (else the filter itself is untrustworthy and the raw data is kept). Returns the possibly
/// filtered rows and how many were dropped.
pub fn filter_market_price_outliers(price_rows: Vec<(f64, Value)>) -> (Vec<(f64, Value)>, usize) {
    if price_rows.len() < 4 {
        return (price_rows, 0);
    }
    let prices: Vec<f64> = price_rows.iter().map(|(p, _)| *p).collect();
    let med = median(&prices);
    if med <= 0.0 {
        return (price_rows, 0);
    }
    let high_cutoff = (med * 4.0).max(med + 1000.0);
    let filtered: Vec<(f64, Value)> = price_rows.iter().filter(|(p, _)| *p <= high_cutoff).cloned().collect();
    if filtered.len() < 3.max(price_rows.len() / 2) {
        return (price_rows, 0);
    }
    let removed = price_rows.len() - filtered.len();
    (filtered, removed)
}

fn rounded_or_none(value: Option<&Value>) -> Option<i64> {
    Some(market::card::round_half_even(value?.as_f64()?))
}

/// A present-but-not-an-object JSON value (e.g. DarkerDB sending a string body) is treated as an
/// empty object rather than propagating: nothing this module reads from a body ever fails outright
/// on an unexpected shape, it just reports "no data" for that field.
fn as_object_or_empty(value: Option<&Value>) -> Value {
    value.filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({}))
}

pub fn missing_key_result(item_name: Option<&str>, rarity: &str) -> Value {
    json!({
        "success": false, "status": "disabled", "has_data": false,
        "error_code": "missing_api_key", "error": "DarkerDB API key is not configured.",
        "item_name": item_name, "rarity": normalize_rarity(rarity),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn request_error_result(item_name: Option<&str>, rarity: &str, status_code: Option<u16>, error_code: &str, message: Option<&str>, rate_limit: Option<Value>) -> Value {
    json!({
        "success": false, "status": "error", "has_data": false,
        "error_code": error_code, "error": message.unwrap_or("Unable to reach DarkerDB market API."),
        "http_status": status_code, "item_name": item_name, "rarity": normalize_rarity(rarity),
        "rate_limit": rate_limit.unwrap_or_else(|| json!({})),
    })
}

/// Maps the legacy (v1) price-check response onto the desktop UI contract.
pub fn normalize_price_check_response(payload: &Value, item_name: &str, rarity: &str, fetched_at: f64, rate_limit: Value) -> Value {
    let body = as_object_or_empty(payload.get("body"));
    let market_price = body.get("market_price").filter(|v| !v.is_null());
    let num_sold = body
        .get("num_similar_sold_recently")
        .or_else(|| body.get("num_listings"))
        .filter(|v| json_truthy(v))
        .and_then(number_as_i64)
        .unwrap_or(0);
    let has_data = market_price.is_some();
    let rounded_price = market_price.and_then(|v| v.as_f64()).map(market::card::round_half_even);
    let updated_at = ["updated_at", "last_updated", "observed_at"]
        .into_iter()
        .find_map(|k| body.get(k).filter(|v| json_truthy(v)))
        .cloned()
        .unwrap_or(Value::Null);

    json!({
        "success": true, "status": "ready", "has_data": has_data, "item_name": item_name,
        "rarity": normalize_rarity(rarity), "avg_price": rounded_price, "min_price": rounded_price,
        "max_price": rounded_price, "recent_price": rounded_price, "num_listings": num_sold,
        "confidence": confidence_level(num_sold), "freshness": freshness(&updated_at, fetched_at),
        "updated_at": updated_at, "fetched_at": iso8601_utc(fetched_at),
        "quality": body.get("quality").cloned().unwrap_or(Value::Null),
        "relative_quality": body.get("relative_quality").cloned().unwrap_or(Value::Null),
        "source": "DarkerDB", "rate_limit": rate_limit, "cache": "miss",
    })
}

/// Maps DarkerDB's v2 exact valuation onto the desktop UI contract.
pub fn normalize_v2_price_check_response(payload: &Value, item_name: &str, rarity: &str, fetched_at: f64, rate_limit: Value) -> Value {
    let envelope = if payload.is_object() { payload.clone() } else { json!({}) };
    let body = as_object_or_empty(envelope.get("body"));
    let valuation = as_object_or_empty(body.get("valuation"));
    let market_info = as_object_or_empty(body.get("market"));
    let item = as_object_or_empty(body.get("item"));
    let similar_sales_count = body.get("similar_sales").and_then(Value::as_array).map_or(0, Vec::len) as i64;
    let similar_listings_count = body.get("similar_listings").and_then(Value::as_array).map_or(0, Vec::len) as i64;

    let fair_value = valuation.get("fair_value").filter(|v| !v.is_null());
    let quick_list = valuation.get("quick_list").filter(|v| !v.is_null());
    let lowest_ask = valuation.get("lowest_ask").filter(|v| !v.is_null());
    let has_data = fair_value.is_some() || quick_list.is_some() || lowest_ask.is_some();

    let sample_count = if similar_sales_count > 0 {
        similar_sales_count
    } else {
        market_info.get("sales_30d").or_else(|| market_info.get("inferred_sales_30d")).filter(|v| json_truthy(v)).and_then(number_as_i64).unwrap_or(0)
    };
    let updated_at = envelope.get("timestamp").cloned().unwrap_or(Value::Null);
    let confidence = valuation
        .get("confidence")
        .filter(|v| json_truthy(v))
        .map_or_else(|| confidence_level(sample_count).to_string(), |v| json_str(v).to_lowercase());
    let active_listings =
        market_info.get("active_listings").filter(|v| json_truthy(v)).and_then(number_as_i64).unwrap_or(similar_listings_count);
    let item_name_out =
        item.get("name").filter(|v| json_truthy(v)).cloned().unwrap_or_else(|| Value::String(item_name.to_string()));
    let rarity_in = item.get("rarity").filter(|v| json_truthy(v)).map_or_else(|| rarity.to_string(), json_str);

    json!({
        "success": true, "status": "ready", "has_data": has_data,
        "item_name": item_name_out, "item_id": item.get("item_id").cloned().unwrap_or(Value::Null),
        "rarity": normalize_rarity(&rarity_in), "avg_price": rounded_or_none(fair_value),
        "min_price": rounded_or_none(valuation.get("low")), "max_price": rounded_or_none(valuation.get("high")),
        "recent_price": rounded_or_none(quick_list.or(lowest_ask)),
        "lowest_ask": rounded_or_none(lowest_ask), "num_listings": sample_count,
        "active_listings": active_listings,
        "confidence": confidence, "freshness": freshness(&updated_at, fetched_at),
        "updated_at": updated_at, "fetched_at": iso8601_utc(fetched_at),
        "quality": Value::Null, "relative_quality": Value::Null,
        "selection": body.get("selection").filter(|v| json_truthy(v)).cloned().unwrap_or_else(|| json!({})),
        "source": "DarkerDB price checks", "rate_limit": rate_limit, "cache": "miss",
    })
}

/// Maps a `/v2/market` listings page onto the desktop UI contract, aggregating price stats across
/// the page (after dropping outliers) rather than reporting one exact valuation.
pub fn normalize_market_listings_response(payload: &Value, item_name: &str, rarity: &str, fetched_at: f64, rate_limit: Value) -> Value {
    let listings = payload.as_object().and_then(|_| payload.get("body")).and_then(Value::as_array);
    let Some(listings) = listings else {
        return request_error_result(
            Some(item_name), rarity, None, "invalid_response", Some("DarkerDB returned an invalid market response."), Some(rate_limit),
        );
    };
    let mut price_rows: Vec<(f64, Value)> = Vec::new();
    for row in listings {
        if !row.is_object() {
            continue;
        }
        let raw = row.get("price_per_unit").filter(|v| json_truthy(v)).or_else(|| row.get("price").filter(|v| json_truthy(v)));
        let price = match raw {
            None => 0.0,
            Some(v) => match as_f64_loose(v) {
                Some(f) => f,
                None => continue,
            },
        };
        if price > 0.0 {
            price_rows.push((price, row.clone()));
        }
    }

    let raw_num_listings = price_rows.len();
    let (filtered_rows, outliers_filtered) = filter_market_price_outliers(price_rows);
    let prices: Vec<f64> = filtered_rows.iter().map(|(p, _)| *p).collect();
    let has_data = !prices.is_empty();

    let mut latest: Option<String> = None;
    let mut latest_price: Option<f64> = None;
    for (price, row) in &filtered_rows {
        let candidate =
            row.get("created_at").filter(|v| json_truthy(v)).or_else(|| row.get("updated_at").filter(|v| json_truthy(v))).and_then(Value::as_str);
        if let Some(candidate) = candidate {
            if latest.as_deref().is_none_or(|l| candidate > l) {
                latest = Some(candidate.to_string());
                latest_price = Some(*price);
            }
        }
    }
    let round = |v: f64| Value::from(market::card::round_half_even(v));
    let recent_price = latest_price.or_else(|| prices.first().copied());

    json!({
        "success": true, "status": "ready", "has_data": has_data, "item_name": item_name,
        "rarity": normalize_rarity(rarity),
        "avg_price": has_data.then(|| round(prices.iter().sum::<f64>() / prices.len() as f64)),
        "min_price": prices.iter().cloned().fold(None::<f64>, |m, p| Some(m.map_or(p, |m| m.min(p)))).map(round),
        "max_price": prices.iter().cloned().fold(None::<f64>, |m, p| Some(m.map_or(p, |m| m.max(p)))).map(round),
        "recent_price": recent_price.map(round),
        "num_listings": prices.len(), "raw_num_listings": raw_num_listings, "outliers_filtered": outliers_filtered,
        "confidence": confidence_level(prices.len() as i64),
        "freshness": freshness(&latest.clone().map_or(Value::Null, Value::String), fetched_at),
        "updated_at": latest, "fetched_at": iso8601_utc(fetched_at),
        "quality": Value::Null, "relative_quality": Value::Null, "source": "DarkerDB market listings",
        "rate_limit": rate_limit, "cache": "miss",
    })
}

/// A one-line verdict across many individual price-check results (e.g. for a bulk-price UI banner):
/// whether DarkerDB is usable at all right now, and why not if not.
pub fn summarize_bulk_price_results(results: &HashMap<String, Value>) -> Value {
    let mut seen = std::collections::HashSet::new();
    let mut unique_values: Vec<&Value> = Vec::new();
    for (index, value) in results.values().enumerate() {
        if !value.is_object() {
            continue;
        }
        let key = value
            .get("cache_key")
            .filter(|v| json_truthy(v))
            .or_else(|| value.get("simple_key").filter(|v| json_truthy(v)))
            .map_or_else(|| format!("#{index}"), json_str);
        if seen.insert(key) {
            unique_values.push(value);
        }
    }
    if unique_values.is_empty() {
        return json!({"success": true, "status": "empty"});
    }
    fn error_code(v: &Value) -> Option<&str> {
        v.get("error_code").and_then(Value::as_str)
    }
    if unique_values.iter().all(|v| error_code(v) == Some("missing_api_key")) {
        return json!({
            "success": false, "status": "disabled", "error_code": "missing_api_key",
            "error": "DarkerDB API key is not configured.",
        });
    }
    if unique_values.iter().any(|v| error_code(v) == Some("rate_limited")) {
        return json!({
            "success": false, "status": "rate_limited", "error_code": "rate_limited",
            "error": "DarkerDB rate limit reached. Try again after the reset window.",
        });
    }
    if unique_values.iter().any(|v| v.get("success").is_some_and(json_truthy)) {
        return json!({"success": true, "status": "ready"});
    }
    json!({"success": false, "status": "error", "error_code": "request_failed", "error": "Unable to reach DarkerDB market data."})
}
