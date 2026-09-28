//! Port of DnDTools' `tests/test_market_service.py`.
//!
//! `_parse_timestamp` is a private implementation detail here (Python tests it directly since the
//! language does not enforce the underscore convention); its two cases — a naive timestamp read as
//! UTC, and an out-of-range one rejected — are instead verified indirectly through the public
//! `normalize_price_check_response`'s `freshness` field, which is exactly what depends on it.

use std::collections::HashMap;
use std::sync::Mutex;

use lister::market_service::{
    build_cache_key, fetch_market_listing_estimate, fetch_market_listings, fetch_price_check, normalize_market_listings_response,
    normalize_price_check_response, summarize_bulk_price_results, HttpError, HttpGet, HttpResponse, MarketPriceCache, DARKERDB_API_VERSION,
};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
struct Call {
    url: String,
    params: HashMap<String, String>,
    headers: HashMap<String, String>,
}

/// A scripted [`HttpGet`]: each call is recorded, then answered by `responder` (given the call
/// index), mirroring the Python tests' `monkeypatch.setattr(market_service.requests, "get", ...)`.
#[allow(clippy::type_complexity)]
struct FakeHttp {
    calls: Mutex<Vec<Call>>,
    responder: Box<dyn Fn(usize, &Call) -> Result<HttpResponse, HttpError> + Send + Sync>,
}

impl FakeHttp {
    fn new(responder: impl Fn(usize, &Call) -> Result<HttpResponse, HttpError> + Send + Sync + 'static) -> Self {
        FakeHttp { calls: Mutex::new(Vec::new()), responder: Box::new(responder) }
    }

    fn once(response: HttpResponse) -> Self {
        Self::new(move |_i, _c| Ok(clone_response(&response)))
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

fn clone_response(r: &HttpResponse) -> HttpResponse {
    HttpResponse { status: r.status, headers: r.headers.clone(), json: r.json.clone() }
}

impl HttpGet for FakeHttp {
    fn get(&self, url: &str, params: &[(String, String)], headers: &HashMap<String, String>, _timeout_s: f64) -> Result<HttpResponse, HttpError> {
        let mut calls = self.calls.lock().unwrap();
        let call = Call { url: url.to_string(), params: params.iter().cloned().collect(), headers: headers.clone() };
        let result = (self.responder)(calls.len(), &call);
        calls.push(call);
        result
    }
}

fn ok_json(body: Value) -> HttpResponse {
    HttpResponse { status: 200, headers: HashMap::new(), json: Ok(body) }
}

fn status_only(status: u16) -> HttpResponse {
    HttpResponse { status, headers: HashMap::new(), json: Ok(json!({"status": "error"})) }
}

fn env(key: &str) -> impl Fn(&str) -> Option<String> {
    let key = key.to_string();
    move |name| (name == "DARKERDB_API_KEY").then(|| key.clone())
}

fn no_key() -> impl Fn(&str) -> Option<String> {
    |_name| None
}

#[test]
fn price_check_request_uses_v2_exact_endpoint_header_key_and_roll_filters() {
    let http = FakeHttp::once(ok_json(json!({
        "timestamp": "2026-08-11T09:01:00+00:00",
        "body": {
            "item": {"item_id": "id.item.frost_amulet_6001", "name": "Frost Amulet", "rarity": "legendary"},
            "valuation": {"fair_value": 123, "low": 100, "high": 150, "quick_list": 119, "confidence": "high"},
            "market": {"active_listings": 5, "sales_30d": 20},
            "similar_sales": [{"price": 120}, {"price": 126}],
        },
    })));
    let cache = MarketPriceCache::new();
    let pp = json!([["AdditionalMoveSpeed", 1.5]]);
    let sp = json!([["Physical Power", "3:"]]);
    let result = fetch_price_check(&http, &cache, &env("test-key"), "Frost Amulet", "Legend", Some(&pp), Some(&sp), "FrostAmulet_6001", "", 0.0, 10.0);

    assert_eq!(result["success"], json!(true));
    assert_eq!(result["avg_price"], json!(123));
    let calls = http.calls();
    assert_eq!(calls[0].url, "https://api.darkerdb.com/v2/price-checks");
    assert!(!calls[0].params.contains_key("key"));
    assert_eq!(calls[0].headers["X-Api-Key"], "test-key");
    assert_eq!(calls[0].headers["X-API-Version"], DARKERDB_API_VERSION);
    assert_eq!(calls[0].params["item_id"], "id.item.frost_amulet_6001");
    assert!(!calls[0].params.contains_key("has_sold"));
    assert_eq!(calls[0].params["attributes[additional_move_speed]"], "1.5");
    assert_eq!(calls[0].params["attributes[physical_power]"], "3:");
    assert!(calls[0].headers["User-Agent"].starts_with("DnDTools-MarketProxy"));
}

#[test]
fn price_check_marks_missing_key_without_calling_api() {
    let http = FakeHttp::new(|_i, _c| panic!("API should not be called without a key"));
    let cache = MarketPriceCache::new();
    let result = fetch_price_check(&http, &cache, &no_key(), "Gold Coin Bag", "Rare", None, None, "", "", 0.0, 10.0);
    assert_eq!(result["success"], json!(false));
    assert_eq!(result["error_code"], json!("missing_api_key"));
    assert_eq!(result["status"], json!("disabled"));
}

#[test]
fn price_check_normalizes_body_confidence_and_price_fields() {
    let now = 1_775_000_000.0;
    let payload = json!({
        "body": {
            "market_price": 456.7, "num_similar_sold_recently": 2, "quality": 82, "relative_quality": 91,
            "updated_at": "2026-07-09T12:00:00+00:00",
        }
    });
    let result = normalize_price_check_response(&payload, "Frost Amulet", "Epic", now, json!({"remaining": "57", "reset": "60"}));
    assert_eq!(result["has_data"], json!(true));
    assert_eq!(result["avg_price"], json!(457));
    assert_eq!(result["recent_price"], json!(457));
    assert_eq!(result["num_listings"], json!(2));
    assert_eq!(result["confidence"], json!("low"));
    assert_eq!(result["quality"], json!(82));
    assert_eq!(result["relative_quality"], json!(91));
    assert_eq!(result["freshness"], json!("fresh"));
    assert_eq!(result["source"], json!("DarkerDB"));
    assert_eq!(result["rate_limit"]["remaining"], json!("57"));
}

/// `2000-01-01T00:00:00` (no offset) must be read as UTC, the same instant as Unix time
/// 946684800 — replaces `test_timestamp_parser_treats_naive_upstream_times_as_utc...`'s direct
/// check on the private parser (see this file's top doc comment).
#[test]
fn normalize_response_treats_a_naive_updated_at_as_utc() {
    const NAIVE_2000_01_01_UTC: f64 = 946_684_800.0;
    let payload = json!({"body": {"market_price": 10, "updated_at": "2000-01-01T00:00:00"}});
    let fresh = normalize_price_check_response(&payload, "x", "", NAIVE_2000_01_01_UTC + 500.0, json!({}));
    assert_eq!(fresh["freshness"], json!("fresh"));
    let recent = normalize_price_check_response(&payload, "x", "", NAIVE_2000_01_01_UTC + 1000.0, json!({}));
    assert_eq!(recent["freshness"], json!("recent"));
}

/// A wildly out-of-range numeric timestamp (`OverflowError` in Python's `datetime.fromtimestamp`)
/// must not be treated as parseable — the other half of the private-parser test (see the top doc
/// comment).
#[test]
fn normalize_response_treats_an_overflowing_updated_at_as_unknown_freshness() {
    let payload = json!({"body": {"market_price": 10, "updated_at": 1e100}});
    let result = normalize_price_check_response(&payload, "x", "", 1_775_000_000.0, json!({}));
    assert_eq!(result["freshness"], json!("unknown"));
}

fn v2_body(item_id: &str, fair_value: i64) -> Value {
    json!({
        "timestamp": "2026-08-11T09:12:00+00:00",
        "body": {
            "item": {"item_id": item_id, "name": "Item", "rarity": "rare"},
            "valuation": {"fair_value": fair_value, "confidence": "high"},
            "similar_sales": (0..12).map(|_| json!({"price": fair_value})).collect::<Vec<_>>(),
        },
    })
}

#[test]
fn price_check_caches_successful_results() {
    let http = FakeHttp::new(|_i, _c| Ok(ok_json(v2_body("id.item.gold_coin_bag", 10))));
    let cache = MarketPriceCache::new();
    let call = |now: f64| fetch_price_check(&http, &cache, &env("test-key"), "Gold Coin Bag", "Rare", None, None, "GoldCoinBag_4001", "", now, 10.0);
    let first = call(100.0);
    let second = call(100.0);
    assert_eq!(http.calls().len(), 1);
    assert_eq!(first["avg_price"], second["avg_price"]);
    assert_eq!(first["avg_price"], json!(10));
    assert_eq!(first["num_listings"], json!(12));
    assert_eq!(second["cache"], json!("hit"));
}

#[test]
fn price_cache_separates_same_name_and_rarity_by_exact_item_id() {
    let http = FakeHttp::new(|_i, call: &Call| {
        let item_id = call.params["item_id"].clone();
        let price = if item_id.ends_with("poison_vial_2001") { 100 } else { 275 };
        Ok(ok_json(v2_body(&item_id, price)))
    });
    let cache = MarketPriceCache::new();
    let first = fetch_price_check(&http, &cache, &env("test-key"), "Poison Vial", "Common", None, None, "PoisonVial_2001", "", 100.0, 10.0);
    let second = fetch_price_check(&http, &cache, &env("test-key"), "Poison Vial", "Common", None, None, "PoisoncloudVial_2001", "", 100.0, 10.0);
    assert_eq!(first["avg_price"], json!(100));
    assert_eq!(second["avg_price"], json!(275));
    let item_ids: Vec<String> = http.calls().iter().map(|c| c.params["item_id"].clone()).collect();
    assert_eq!(item_ids, vec!["id.item.poison_vial_2001".to_string(), "id.item.poisoncloud_vial_2001".to_string()]);
}

#[test]
fn market_cache_is_bounded_and_prunes_expired_entries() {
    let cache = MarketPriceCache::with_limits(10.0, 3);
    for i in 0..5 {
        cache.insert(format!("item-{i}"), json!({"success": true, "value": i}), 100.0 + i as f64);
    }
    assert_eq!(cache.len(), 3);
    assert!(cache.get("item-0", 104.0).is_none());
    assert!(cache.get("item-1", 104.0).is_none());
    for i in 2..5 {
        assert!(cache.get(&format!("item-{i}"), 104.0).is_some());
    }
    assert!(cache.get("item-4", 200.0).is_none());
    assert_eq!(cache.len(), 0);
}

#[test]
fn market_listing_request_uses_v2_endpoint_and_envelope() {
    let http = FakeHttp::once(ok_json(json!({
        "body": [{"id": "listing-1", "item_id": "id.item.potion_health", "name": "Potion of Healing", "price": 25,
                  "created_at": "2026-07-09T12:00:00+00:00"}],
        "pagination": {"total": 1},
    })));
    let result = fetch_market_listings(&http, &env("test-key"), "id.item.potion_health", "", "Rare", Some(5), true, None, 10.0);
    assert_eq!(result["success"], json!(true));
    let calls = http.calls();
    assert_eq!(calls[0].url, "https://api.darkerdb.com/v2/market");
    assert!(!calls[0].params.contains_key("key"));
    assert_eq!(calls[0].params["item_id"], "id.item.potion_health");
    assert_eq!(calls[0].params["listing_state"], "sold");
    assert!(!calls[0].params.contains_key("has_sold"));
    assert_eq!(calls[0].params["limit"], "5");
    assert_eq!(result["pagination"]["total"], json!(1));
    assert_eq!(result["listings"][0]["price"], json!(25));
}

#[test]
fn price_check_uses_v2_exact_quote_for_dndtools_item_id() {
    let http = FakeHttp::once(ok_json(json!({
        "timestamp": "2026-08-23T12:01:00+00:00",
        "body": {
            "item": {"item_id": "id.item.adventurer_boots_2001", "name": "Adventurer Boots", "rarity": "common"},
            "valuation": {"fair_value": 120, "low": 100, "high": 140, "quick_list": 115, "lowest_ask": 110, "confidence": "medium"},
            "market": {"active_listings": 3, "sales_30d": 9},
            "similar_sales": [{"price": 100}, {"price": 140}],
        },
    })));
    let cache = MarketPriceCache::new();
    let result = fetch_price_check(&http, &cache, &env("test-key"), "Adventurer Boots", "Common", None, None, "AdventurerBoots_2001", "", 100.0, 10.0);
    assert_eq!(result["success"], json!(true));
    assert_eq!(result["has_data"], json!(true));
    assert_eq!(result["avg_price"], json!(120));
    assert_eq!(result["min_price"], json!(100));
    assert_eq!(result["max_price"], json!(140));
    assert_eq!(result["num_listings"], json!(2));
    assert_eq!(http.calls().len(), 1);
    assert_eq!(result["recent_price"], json!(115));
    let calls = http.calls();
    assert_eq!(calls[0].url, "https://api.darkerdb.com/v2/price-checks");
    assert_eq!(calls[0].params["item_id"], "id.item.adventurer_boots_2001");
    assert_eq!(calls[0].headers["X-Api-Key"], "test-key");
}

#[test]
fn exact_price_check_falls_back_to_v2_market_for_unknown_variant() {
    let http = FakeHttp::new(|_i, call: &Call| {
        if call.url.ends_with("/v2/price-checks") {
            Ok(status_only(404))
        } else {
            Ok(ok_json(json!({"body": [{"price": 75, "created_at": "2026-08-23T12:00:00+00:00"}]})))
        }
    });
    let cache = MarketPriceCache::new();
    let result = fetch_price_check(&http, &cache, &env("test-key"), "Unknown Boots", "Rare", None, None, "UnknownBoots_4001", "", 100.0, 10.0);
    assert_eq!(result["success"], json!(true));
    assert_eq!(result["avg_price"], json!(75));
    let urls: Vec<String> = http.calls().iter().map(|c| c.url.clone()).collect();
    assert_eq!(urls, vec!["https://api.darkerdb.com/v2/price-checks".to_string(), "https://api.darkerdb.com/v2/market".to_string()]);
}

#[test]
fn listing_estimate_filters_extreme_high_outliers() {
    let payload = json!({
        "body": [
            {"price": 100, "created_at": "2026-07-09T12:00:00+00:00"},
            {"price": 110, "created_at": "2026-07-09T12:01:00+00:00"},
            {"price": 120, "created_at": "2026-07-09T12:02:00+00:00"},
            {"price": 130, "created_at": "2026-07-09T12:03:00+00:00"},
            {"price": 53000, "created_at": "2026-07-09T12:04:00+00:00"},
        ]
    });
    let result = normalize_market_listings_response(&payload, "Adventurer Boots", "Common", 1_775_000_000.0, json!({}));
    assert_eq!(result["avg_price"], json!(115));
    assert_eq!(result["recent_price"], json!(130));
    assert_eq!(result["max_price"], json!(130));
    assert_eq!(result["num_listings"], json!(4));
    assert_eq!(result["outliers_filtered"], json!(1));
}

#[test]
fn bulk_status_reports_missing_key_when_every_result_is_disabled() {
    let disabled = || json!({"success": false, "status": "disabled", "error_code": "missing_api_key", "error": "DarkerDB API key is not configured."});
    let results = HashMap::from([("Gold Coin Bag|Rare".to_string(), disabled()), ("Frost Amulet|Legendary".to_string(), disabled())]);
    let status = summarize_bulk_price_results(&results);
    assert_eq!(status["success"], json!(false));
    assert_eq!(status["status"], json!("disabled"));
    assert_eq!(status["error_code"], json!("missing_api_key"));
}

#[test]
fn cache_key_is_canonical_for_roll_order_rarity_alias_and_identity() {
    let pp = [("Strength".to_string(), json!(1)), ("MoveSpeed".to_string(), json!(2))];
    let sp = [("Physical Power".to_string(), json!(3))];
    let key = build_cache_key("Frost Amulet", "Legend", &pp, &sp, "FrostAmulet_6001", "");
    assert_eq!(key, "Frost Amulet|Legendary|id:frostamulet_6001|p:move_speed=2|p:strength=1|s:physical_power=3");
}

#[test]
fn price_check_handles_success_with_malformed_json() {
    let http = FakeHttp::once(HttpResponse { status: 200, headers: HashMap::new(), json: Err("bad json".to_string()) });
    let cache = MarketPriceCache::new();
    let result = fetch_price_check(&http, &cache, &env("test-key"), "Frost Amulet", "Epic", None, None, "FrostAmulet_5001", "", 100.0, 10.0);
    assert_eq!(result["success"], json!(false));
    assert_eq!(result["error_code"], json!("invalid_response"));
}

#[test]
fn market_listings_rejects_invalid_success_envelope() {
    let http = FakeHttp::once(ok_json(json!({"body": {"not": "a list"}})));
    let result = fetch_market_listings(&http, &env("test-key"), "id.item.frost_amulet_5001", "", "", None, true, None, 10.0);
    assert_eq!(result["success"], json!(false));
    assert_eq!(result["error_code"], json!("invalid_response"));
}

#[test]
fn price_check_rejects_malformed_or_unbounded_rolls_before_network() {
    let http = FakeHttp::new(|_i, _c| panic!("invalid rolls must not reach the network"));
    let cache = MarketPriceCache::new();
    let malformed_pp = json!([{"name": "Strength"}]);
    let malformed = fetch_price_check(&http, &cache, &env("test-key"), "Frost Amulet", "Epic", Some(&malformed_pp), None, "FrostAmulet_5001", "", 100.0, 10.0);
    let oversized_sp = Value::Array(vec![json!(["Strength", 1]); 65]);
    let oversized = fetch_price_check(&http, &cache, &env("test-key"), "Frost Amulet", "Epic", None, Some(&oversized_sp), "FrostAmulet_5001", "", 100.0, 10.0);
    assert_eq!(malformed["error_code"], json!("invalid_request"));
    assert_eq!(oversized["error_code"], json!("invalid_request"));
}

/// Not exercised via `fetch_price_check`'s fallback (which always has an item id by the time it
/// falls through): a direct call with neither an item id nor an archetype has nothing to search
/// for at all.
#[test]
fn fetch_market_listing_estimate_without_any_id_reports_a_helpful_error() {
    let http = FakeHttp::new(|_i, _c| panic!("no id to search for; must not reach the network"));
    let result = fetch_market_listing_estimate(&http, &env("test-key"), "Mystery Item", "", "", "", &[], &[], 100.0, 10.0);
    assert_eq!(result["success"], json!(false));
    assert_eq!(result["error"], json!("No DarkerDB item id available for market lookup."));
}
