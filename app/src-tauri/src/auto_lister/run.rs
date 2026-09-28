//! Runs that click in the game (list, price from the game, collect, crawl, sell to a merchant) and
//! what the page polls while they go. Only one run at a time; Ctrl+F12 stops it.

use std::collections::HashSet;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use lister::job::{ItemResult, Mode, RunState};
use lister::merchant_seller::{merchant_value, resolve_sell_entries};

use super::plan_view::{parse_entries, EntryView, ListingsInfo, PlanView};
use super::reprice::Repricer;
use super::runtime::ListerRuntime;
use super::{find_character, is_stale, listings_info, load_rules, stash};
use crate::state::AppState;

/// The merchant the lister sells to.
const MERCHANT_NAME: &str = "The Collector";
/// One stash tab's worth: the most a single sale may take.
const MAX_MERCHANT_ITEMS: usize = 240;
/// "Update market data": pages read, stopping early at pages already recorded.
const UPDATE_CRAWL_PAGES: u32 = 20;
/// "Deep crawl": pages of gear read per rarity, all of them.
const DEEP_CRAWL_PAGES: u32 = 60;
/// An incremental crawl stops at a page this share of which it had already recorded.
const OLD_PAGE_SHARE: f64 = 0.8;
const ALREADY_RUNNING: &str = "The lister is already running.";
const STALE_AFTER_SALE: &str = "The app hasn't seen your stash since the last sale (it still shows items already sold), \
     so item positions may be wrong: reopen your character to refresh, then try again.";
const ALREADY_SOLD: &str = "already sold";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemResultView {
    unique_id: String,
    name: String,
    status: String,
    message: String,
}

impl From<&ItemResult> for ItemResultView {
    fn from(r: &ItemResult) -> Self {
        ItemResultView { unique_id: r.unique_id.clone(), name: r.name.clone(), status: r.status.clone(), message: r.message.clone() }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    state: &'static str,
    mode: Option<&'static str>,
    results: Vec<ItemResultView>,
    stopped_reason: Option<String>,
    plan: Option<PlanView>,
    listings: ListingsInfo,
    total: usize,
    can_run: bool,
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::List => "list",
        Mode::DryRun => "dryRun",
        Mode::Price => "price",
        Mode::Crawl => "crawl",
        Mode::Collect => "collect",
        Mode::Merchant => "merchant",
        Mode::MerchantDryRun => "merchantDryRun",
        Mode::Hover => "hover",
    }
}

#[tauri::command]
pub fn lister_status(state: State<'_, AppState>, runtime: State<'_, ListerRuntime>) -> StatusView {
    let status = runtime.job.status();
    StatusView {
        state: match status.state {
            RunState::Idle => "idle",
            RunState::Running => "running",
            RunState::Done => "done",
        },
        mode: status.mode.map(mode_name),
        results: status
            .results
            .iter()
            .map(|result| {
                // Collect reports carry the item's id; show its name like every other result.
                let mut view = ItemResultView::from(result);
                if let Some(item) = state.catalog.get(&view.name) {
                    view.name = item.name.clone();
                }
                view
            })
            .collect(),
        stopped_reason: status.stopped_reason,
        plan: status.plan.as_ref().map(PlanView::from),
        listings: listings_info(&state.marketplace),
        total: runtime.total(),
        can_run: true,
    }
}

/// Takes the mouse for one lister run: `start` launches it (false when one is already going), and
/// the lock is held until it finishes.
fn launch(state: &AppState, runtime: &ListerRuntime, start: impl FnOnce() -> bool) -> Result<(), String> {
    let guard = state.input_lock.try_acquire(super::runtime::LISTER_NAME)?;
    if !start() {
        return Err(ALREADY_RUNNING.into());
    }
    runtime.hold_until_done(guard);
    Ok(())
}

/// Searches the market in game for each entry, then prices the plan from what it found.
#[tauri::command]
pub fn lister_price_from_game(app: AppHandle, runtime: State<'_, ListerRuntime>, entries: Vec<EntryView>) -> Result<(), String> {
    let entries = parse_entries(entries, "price", true)?;
    let rules = load_rules(&app.state::<AppState>());
    runtime.prepare(Vec::new(), entries.len());
    let pricing_app = app.clone();
    launch(&app.state::<AppState>(), &runtime, || {
        runtime.job.price(entries, move |entries, rows| {
            let state = pricing_app.state::<AppState>();
            super::pricing_for(&state).game_prices(entries, rows, &rules)
        })
    })
}

/// Lists the approved entries (or, as a dry run, goes through every step without listing).
#[tauri::command]
pub fn lister_start(
    app: AppHandle,
    runtime: State<'_, ListerRuntime>,
    character_id: String,
    entries: Vec<EntryView>,
    dry_run: bool,
    recheck: bool,
) -> Result<(), String> {
    let entries = parse_entries(entries, "list", false)?;
    let state = app.state::<AppState>();
    let character = find_character(&state, &character_id)?;
    let rules = load_rules(&state);
    runtime.prepare(tab_mapping(&character), entries.len());
    let reprice: Option<Arc<dyn lister::job::Reprice>> = recheck.then(|| Arc::new(Repricer { app: app.clone(), rules }) as _);
    launch(&state, &runtime, || runtime.job.start(entries, dry_run, reprice))
}

/// The character's stash tabs in Marketplace icon order.
fn tab_mapping(character: &state::Character) -> Vec<i32> {
    stash::tab_mapping(character).into_iter().filter_map(|id| i32::try_from(id).ok()).collect()
}

/// Stops whatever the lister is doing; false when nothing was running.
#[tauri::command]
pub fn lister_stop(runtime: State<'_, ListerRuntime>) -> bool {
    runtime.job.cancel()
}

/// Collects the gold of sold listings and takes back expired ones.
#[tauri::command]
pub fn lister_collect(state: State<'_, AppState>, runtime: State<'_, ListerRuntime>) -> Result<(), String> {
    runtime.prepare(Vec::new(), 0);
    launch(&state, &runtime, || runtime.job.collect())
}

/// Reads Marketplace pages into the market data: new listings only, or a deep crawl of every rarity.
#[tauri::command]
pub fn lister_crawl(app: AppHandle, runtime: State<'_, ListerRuntime>, deep: bool) -> Result<(), String> {
    let pages = if deep { DEEP_CRAWL_PAGES } else { UPDATE_CRAWL_PAGES };
    runtime.prepare(Vec::new(), pages as usize);
    let is_old_page = (!deep).then(|| {
        let (app, started) = (app.clone(), crate::hover::facts::now_s());
        Arc::new(move |rows: &lister::job::CrawlPage| {
            let ids: Vec<String> = rows.iter().map(|row| row.listing_id.clone()).filter(|id| !id.is_empty()).collect();
            let state = app.state::<AppState>();
            let Some(market) = state.market.as_ref() else { return false };
            let seen = market.count_seen_before(&ids, started).unwrap_or(0);
            !ids.is_empty() && seen as f64 >= OLD_PAGE_SHARE * ids.len() as f64
        }) as Arc<lister::job::OldPageCheck>
    });
    launch(&app.state::<AppState>(), &runtime, || runtime.job.crawl(pages, is_old_page))
}

/// The calibration check: the mouse rests on each Marketplace spot for a second, no clicks.
#[tauri::command]
pub fn lister_hover_test(state: State<'_, AppState>, runtime: State<'_, ListerRuntime>) -> Result<(), String> {
    runtime.prepare(Vec::new(), 0);
    launch(&state, &runtime, || runtime.job.hover_test())
}

/// Sells the chosen items to the merchant, found in the stash as it is right now.
#[tauri::command]
pub fn lister_sell_to_merchant(
    app: AppHandle,
    runtime: State<'_, ListerRuntime>,
    character_id: String,
    unique_ids: Vec<String>,
    dry_run: bool,
) -> Result<(), String> {
    if unique_ids.is_empty() {
        return Err("Tick at least one item to sell.".into());
    }
    if unique_ids.len() > MAX_MERCHANT_ITEMS {
        return Err(format!("At most {MAX_MERCHANT_ITEMS} items can be sold at once."));
    }
    let state = app.state::<AppState>();
    let rules = load_rules(&state);
    let character = find_character(&state, &character_id)?;
    let stashes = stash::stashes_json(&character, &state.catalog, &rules.source_stash_ids);
    let (entries, refused) = resolve_sell_entries(&stashes, &unique_ids, &rules.source_stash_ids);
    let sold = runtime.job.sold_ids();
    let in_stash: HashSet<String> = character.items.iter().map(|i| i.unique_id.to_string()).collect();
    if sold.iter().any(|id| in_stash.contains(id)) {
        return Err(STALE_AFTER_SALE.into());
    }
    let entries: Vec<_> = entries.into_iter().filter(|e| !sold.contains(&e.unique_id)).collect();
    if entries.is_empty() {
        let reasons: std::collections::BTreeSet<&str> = refused.iter().map(|(_, reason)| reason.as_str()).collect();
        let reasons = if reasons.is_empty() { ALREADY_SOLD.to_string() } else { reasons.into_iter().collect::<Vec<_>>().join("; ") };
        return Err(format!("Nothing can be sold: {reasons}"));
    }
    runtime.prepare(tab_mapping(&character), entries.len());
    launch(&state, &runtime, || runtime.job.sell_to_merchant(entries, dry_run))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantEntryView {
    unique_id: String,
    name: String,
    stash_id: String,
    quantity: i64,
    vendor_price: i64,
    value: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefusedView {
    unique_id: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantPlanView {
    merchant: &'static str,
    entries: Vec<MerchantEntryView>,
    refused: Vec<RefusedView>,
    total: i64,
    warnings: Vec<String>,
}

/// What the merchant pays for the chosen items, looked up in the stash as it is right now.
#[tauri::command]
pub fn lister_merchant_plan(
    state: State<'_, AppState>,
    character_id: String,
    unique_ids: Vec<String>,
) -> Result<MerchantPlanView, String> {
    if unique_ids.is_empty() {
        return Err("Tick at least one item to sell.".into());
    }
    if unique_ids.len() > MAX_MERCHANT_ITEMS {
        return Err(format!(
            "At most {MAX_MERCHANT_ITEMS} items can be sold at once."
        ));
    }
    let rules = load_rules(&state);
    let character = find_character(&state, &character_id)?;
    let stashes = stash::stashes_json(&character, &state.catalog, &rules.source_stash_ids);
    let (entries, refused) = resolve_sell_entries(&stashes, &unique_ids, &rules.source_stash_ids);
    let entries: Vec<MerchantEntryView> = entries
        .iter()
        .map(|e| MerchantEntryView {
            unique_id: e.unique_id.clone(),
            name: e.name.clone(),
            stash_id: e.stash_id.clone(),
            quantity: e.quantity,
            vendor_price: e.vendor_price,
            value: merchant_value(e),
        })
        .collect();
    let mut warnings = Vec::new();
    if is_stale(super::character_data_age(&state, &character_id)) {
        warnings.push(
            "Stash data is more than 5 minutes old: reopen your character in the game to refresh."
                .into(),
        );
    }
    Ok(MerchantPlanView {
        merchant: MERCHANT_NAME,
        total: entries.iter().map(|e| e.value).sum(),
        entries,
        refused: refused
            .into_iter()
            .map(|(unique_id, reason)| RefusedView { unique_id, reason })
            .collect(),
        warnings,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDataView {
    listings: u64,
    items: u64,
    vanished: u64,
    my_sold: u64,
}

#[tauri::command]
pub fn market_data_summary(state: State<'_, AppState>) -> Result<MarketDataView, String> {
    let market = state.market.as_ref().ok_or("Market data is unavailable.")?;
    let summary = market.summary().map_err(|err| err.to_string())?;
    Ok(MarketDataView {
        listings: summary.listings,
        items: summary.items,
        vanished: summary.vanished,
        my_sold: summary.my_sold,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorthInfoView {
    trained: bool,
    listings: u64,
    items: u64,
    trained_at: Option<u64>,
    mdape: Option<f64>,
}

#[tauri::command]
pub fn worth_info(state: State<'_, AppState>) -> WorthInfoView {
    worth_info_of(&state)
}

fn worth_info_of(state: &AppState) -> WorthInfoView {
    let trained_at = std::fs::metadata(state.data.data().join(crate::state::WORTH_MODEL))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let model = state.worth();
    WorthInfoView {
        trained: model.is_some(),
        listings: model.as_deref().map_or(0, market::WorthModel::listings),
        items: 0,
        trained_at,
        mdape: state.worth_error(),
    }
}

/// Retrains the value model on all saved market data (a few seconds, off the main thread).
#[tauri::command]
pub async fn train_worth(app: tauri::AppHandle) -> Result<WorthInfoView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        if state.training.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err("Training is already running.".to_string());
        }
        // A failed pattern analysis keeps the previous patterns; the value model still trains.
        if let Err(err) = crate::training::analyze_market(&state) {
            log::warn!("market patterns were not updated: {err}");
        }
        let trained = crate::training::train_worth_model(&state);
        state.training.store(false, std::sync::atomic::Ordering::SeqCst);
        trained.map(|_| worth_info_of(&state))
    })
    .await
    .map_err(|err| err.to_string())?
}
