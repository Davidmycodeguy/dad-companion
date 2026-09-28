//! The DarkerDB HTTP calls: request orchestration behind [`HttpGet`], so this crate never depends
//! on a concrete HTTP client.
//!
//! Port of `market_service.py`'s `fetch_price_check`, `fetch_market_listing_estimate` and
//! `fetch_market_listings`.

use std::collections::HashMap;

use serde_json::Value;

use super::{attribute_key, build_cache_key, build_v2_price_check_params, darkerdb_api_key, darkerdb_headers, MarketPriceCache};
use super::{json_str, json_truthy, missing_key_result, normalize_market_properties, normalize_rarity};
use super::{normalize_v2_price_check_response, rate_limit_from_headers, request_error_result};
use super::{to_darkerdb_archetype, to_darkerdb_item_id};
use super::{DARKERDB_BASE_URL, MARKET_LISTINGS_PATH, PRICE_CHECK_PATH};

/// Raised by [`HttpGet::get`] for a transport-level failure (DNS, connection refused, timeout, ...)
/// — anything short of getting a response back at all. Mirrors `requests.RequestException`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(pub String);

/// One HTTP response, already read: DnDTools' Python tests hand `fetch_price_check` etc. a fake
/// `session` whose `.get()` returns an object with `.status_code`, `.headers`, `.ok` and `.json()`;
/// this is that same shape.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    /// The parsed JSON body, or the message `response.json()` would have raised, for a body that
    /// was read successfully but was not valid JSON.
    pub json: Result<Value, String>,
}

impl HttpResponse {
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A single blocking HTTP GET, injected so callers choose their client (reqwest, ureq, or a test
/// fake) without this crate depending on one — mirrors the `session` parameter DnDTools' Python
/// accepts for the same reason (defaulting there to the `requests` module).
pub trait HttpGet: Send + Sync {
    fn get(&self, url: &str, params: &[(String, String)], headers: &HashMap<String, String>, timeout_s: f64) -> Result<HttpResponse, HttpError>;
}

/// Query params and a request for `/v2/market`, used both for the listings-estimate fallback and
/// for a plain listings browse.
#[allow(clippy::too_many_arguments)]
fn market_listings_request(
    client: &dyn HttpGet,
    api_key: &str,
    item_id: &str,
    archetype: &str,
    rarity: &str,
    extra_params: &[(String, String)],
    timeout_s: f64,
) -> Result<HttpResponse, HttpError> {
    let mut params = extra_params.to_vec();
    if !item_id.is_empty() {
        params.push(("item_id".to_string(), item_id.to_string()));
    } else if !archetype.is_empty() {
        params.push(("archetype".to_string(), archetype.to_string()));
    }
    if !rarity.is_empty() {
        params.push(("rarity".to_string(), normalize_rarity(rarity).to_lowercase()));
    }
    client.get(&format!("{DARKERDB_BASE_URL}{MARKET_LISTINGS_PATH}"), &params, &darkerdb_headers(api_key), timeout_s)
}

/// Browses `/v2/market` listings directly (not a valuation): `limit` is clamped to `[1, 50]`,
/// defaulting to 10 when `None` or `0`. `price`, when given, is passed through as an extra filter.
#[allow(clippy::too_many_arguments)]
pub fn fetch_market_listings(
    client: &dyn HttpGet,
    get_env: &dyn Fn(&str) -> Option<String>,
    item_id: &str,
    archetype: &str,
    rarity: &str,
    limit: Option<i64>,
    has_sold: bool,
    price: Option<&str>,
    timeout_s: f64,
) -> Value {
    let api_key = darkerdb_api_key(get_env);
    if api_key.is_empty() {
        return missing_key_result(None, "");
    }
    let normalized_limit = limit.filter(|&l| l != 0).unwrap_or(10).clamp(1, 50);
    let mut extra = vec![("limit".to_string(), normalized_limit.to_string())];
    if has_sold {
        extra.push(("listing_state".to_string(), "sold".to_string()));
    }
    if let Some(price) = price.filter(|p| !p.is_empty()) {
        extra.push(("price".to_string(), price.to_string()));
    }
    let response = match market_listings_request(client, &api_key, item_id, archetype, rarity, &extra, timeout_s) {
        Ok(r) => r,
        Err(_) => return request_error_result(item_id_or_archetype(item_id, archetype), rarity, None, "request_failed", None, None),
    };
    let rate_limit = rate_limit_from_headers(&response.headers);
    if !response.is_ok() {
        let code = if response.status == 429 { "rate_limited" } else { "request_failed" };
        return request_error_result(item_id_or_archetype(item_id, archetype), rarity, Some(response.status), code, None, Some(rate_limit));
    }
    let Ok(payload) = response.json else {
        return request_error_result(
            item_id_or_archetype(item_id, archetype), rarity, Some(response.status), "invalid_response",
            Some("DarkerDB returned an invalid market response."), Some(rate_limit),
        );
    };
    let Some(body) = payload.get("body").filter(|b| b.is_array()) else {
        return request_error_result(
            item_id_or_archetype(item_id, archetype), rarity, Some(response.status), "invalid_response",
            Some("DarkerDB returned an invalid market response."), Some(rate_limit),
        );
    };
    let pagination = payload.get("pagination").filter(|p| p.is_object()).cloned().unwrap_or_else(|| serde_json::json!({}));
    serde_json::json!({
        "success": true, "status": "ready", "listings": body, "pagination": pagination,
        "rate_limit": rate_limit, "source": "DarkerDB",
    })
}

fn item_id_or_archetype<'a>(item_id: &'a str, archetype: &'a str) -> Option<&'a str> {
    Some(item_id).filter(|s| !s.is_empty()).or_else(|| Some(archetype).filter(|s| !s.is_empty()))
}

/// Values a legacy or unrecognised-variant item from recent sold listings, since no exact
/// valuation is available for it: the `/v2/market` fallback `fetch_price_check` uses when the v2
/// price-check endpoint has nothing for the item's precise id.
#[allow(clippy::too_many_arguments)]
pub fn fetch_market_listing_estimate(
    client: &dyn HttpGet,
    get_env: &dyn Fn(&str) -> Option<String>,
    item_name: &str,
    rarity: &str,
    item_id: &str,
    archetype: &str,
    pp: &[(String, Value)],
    sp: &[(String, Value)],
    now: f64,
    timeout_s: f64,
) -> Value {
    let api_key = darkerdb_api_key(get_env);
    if api_key.is_empty() {
        return missing_key_result(Some(item_name), rarity);
    }
    let darkerdb_item_id = to_darkerdb_item_id(item_id);
    let darkerdb_archetype = to_darkerdb_archetype(item_id, archetype);
    if darkerdb_item_id.is_empty() && darkerdb_archetype.is_empty() {
        return request_error_result(
            Some(item_name), rarity, Some(404), "request_failed", Some("No DarkerDB item id available for market lookup."), None,
        );
    }
    let mut extra = vec![("limit".to_string(), "50".to_string()), ("listing_state".to_string(), "sold".to_string())];
    if !darkerdb_item_id.is_empty() {
        extra.push(("item_id".to_string(), darkerdb_item_id));
    } else {
        extra.push(("archetype".to_string(), darkerdb_archetype));
    }
    for (name, value) in pp {
        extra.push((format!("primary[{}]", attribute_key(name)), json_str(value)));
    }
    for (name, value) in sp {
        extra.push((format!("secondary[{}]", attribute_key(name)), json_str(value)));
    }

    // item_id/archetype/rarity are already folded into `extra` above (DarkerDB-encoded, unlike
    // `market_listings_request`'s own raw-value handling for `fetch_market_listings`), so none are
    // passed here.
    let response = match market_listings_request(client, &api_key, "", "", "", &extra, timeout_s) {
        Ok(r) => r,
        Err(_) => return request_error_result(Some(item_name), rarity, None, "request_failed", None, None),
    };
    let rate_limit = rate_limit_from_headers(&response.headers);
    if !response.is_ok() {
        let code = if response.status == 429 { "rate_limited" } else { "request_failed" };
        return request_error_result(Some(item_name), rarity, Some(response.status), code, None, Some(rate_limit));
    }
    let Ok(payload) = response.json else {
        return request_error_result(
            Some(item_name), rarity, Some(response.status), "invalid_response",
            Some("DarkerDB returned an invalid market response."), Some(rate_limit),
        );
    };
    super::normalize_market_listings_response(&payload, item_name, rarity, now, rate_limit)
}

/// Tries the v2 exact price-check endpoint. `None` means "fall through to the listings estimate":
/// DarkerDB has no exact quote for this variant (400/404/422), which is expected for legacy
/// captures and unrecognised roll names, not a failure worth reporting on its own.
#[allow(clippy::too_many_arguments)]
fn price_check_v2(
    client: &dyn HttpGet,
    darkerdb_item_id: &str,
    pp: &[(String, Value)],
    sp: &[(String, Value)],
    api_key: &str,
    item_name: &str,
    normalized_rarity: &str,
    now: f64,
    timeout_s: f64,
) -> Option<Value> {
    let params = build_v2_price_check_params(darkerdb_item_id, pp, sp);
    let response = match client.get(&format!("{DARKERDB_BASE_URL}{PRICE_CHECK_PATH}"), &params, &darkerdb_headers(api_key), timeout_s) {
        Ok(r) => r,
        Err(_) => return Some(request_error_result(Some(item_name), normalized_rarity, None, "request_failed", None, None)),
    };
    let rate_limit = rate_limit_from_headers(&response.headers);
    if response.is_ok() {
        let valid_envelope = response.json.as_ref().ok().filter(|p| p.is_object() && p.get("body").is_some_and(Value::is_object));
        return Some(match valid_envelope {
            Some(payload) => normalize_v2_price_check_response(payload, item_name, normalized_rarity, now, rate_limit),
            None => request_error_result(
                Some(item_name), normalized_rarity, Some(response.status), "invalid_response",
                Some("DarkerDB returned an invalid price-check response."), Some(rate_limit),
            ),
        });
    }
    if [400, 404, 422].contains(&response.status) {
        return None;
    }
    let code = if response.status == 429 { "rate_limited" } else { "request_failed" };
    Some(request_error_result(Some(item_name), normalized_rarity, Some(response.status), code, None, Some(rate_limit)))
}

/// Values an exact roll via the v2 price-check endpoint, with a `/v2/market` listings-estimate
/// fallback for variants it has no exact quote for. Successful results are cached (keyed on the
/// exact roll combination) for [`super::MARKET_CACHE_DURATION_S`] seconds.
#[allow(clippy::too_many_arguments)]
pub fn fetch_price_check(
    client: &dyn HttpGet,
    cache: &MarketPriceCache,
    get_env: &dyn Fn(&str) -> Option<String>,
    item_name: &str,
    rarity: &str,
    pp: Option<&Value>,
    sp: Option<&Value>,
    item_id: &str,
    archetype: &str,
    now: f64,
    timeout_s: f64,
) -> Value {
    let pp = match normalize_market_properties(pp, 64) {
        Ok(v) => v,
        Err(e) => return request_error_result(Some(item_name), rarity, None, "invalid_request", Some(e.0), None),
    };
    let sp = match normalize_market_properties(sp, 64) {
        Ok(v) => v,
        Err(e) => return request_error_result(Some(item_name), rarity, None, "invalid_request", Some(e.0), None),
    };
    let api_key = darkerdb_api_key(get_env);
    if api_key.is_empty() {
        return missing_key_result(Some(item_name), rarity);
    }
    let normalized_rarity = normalize_rarity(rarity);
    let cache_key = build_cache_key(item_name, &normalized_rarity, &pp, &sp, item_id, archetype);
    if let Some(mut cached) = cache.get(&cache_key, now) {
        if let Value::Object(map) = &mut cached {
            map.insert("cache".to_string(), Value::String("hit".to_string()));
        }
        return cached;
    }

    let darkerdb_item_id = to_darkerdb_item_id(item_id);
    let v2_result = (!darkerdb_item_id.is_empty())
        .then(|| price_check_v2(client, &darkerdb_item_id, &pp, &sp, &api_key, item_name, &normalized_rarity, now, timeout_s))
        .flatten();
    let result = match v2_result {
        Some(r) => r,
        None => fetch_market_listing_estimate(client, get_env, item_name, &normalized_rarity, item_id, archetype, &pp, &sp, now, timeout_s),
    };
    if result.get("success").is_some_and(json_truthy) {
        cache.insert(cache_key, result.clone(), now);
    }
    result
}
