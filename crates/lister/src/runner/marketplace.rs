//! Drives the Marketplace: list plan entries, search the market for them, crawl View Market and
//! collect payouts. Port of DnDTools' `src/models/marketplace_runner.py`.
//!
//! Every flow starts from a confirmed My Listings: nothing is clicked unless My Listings was seen in
//! the last [`MAX_SNAPSHOT_AGE_S`] seconds, and then it is re-opened (via View Market) and the game
//! must answer with a fresh snapshot before anything else happens. The flows themselves live in
//! the private modules `listing` (`run`), `search` (`price_all`, `crawl_market`) and `payouts`
//! (`collect_payouts`); this module holds the runner and the steps they share.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use input::marketplace::{spot_location, tab_icon_index, MarketplaceLayout};
use market::MarketRow;

use super::{
    check_cancel, cursor_moved_from, is_off_limits_stash, item_centre, off_limits_message, refused, safety_checkpoint,
    safety_stop_message, unexpected, unmapped_message, GameInput, Halt, MarketplaceGame, NullSafety, SafetyCheck,
    ScreenPoint, Step, MOUSE_MOVED,
};
use crate::job::{CancelToken, ItemResult, OldPageCheck, Reprice, RunReport, Runner};
use crate::marketplace_state::{ListingsSnapshot, FIRST_PAGE, MAX_SNAPSHOT_AGE_S};
use crate::plan::{MarketBucket, PlanEntry};

mod listing;
mod payouts;
mod search;

pub use payouts::listing_pages;

/// My Listings pages (10 spots each).
pub const MAX_PAGES: i64 = 4;
/// Pauses between opening a screen and clicking on it.
pub const SEARCH_SETTLE_PAUSES: usize = 4;
pub const MAX_PAYOUTS_PER_RUN: usize = 40;
/// Barbarian .. Wizard in the View Market class filter.
pub const CLASS_COUNT: i32 = 10;
pub const NO_FREE_SPOTS: &str = "No free listing spots left.";
pub const NOT_ON_MY_LISTINGS: &str =
    "Couldn't confirm My Listings is open — open Trade → Marketplace → My Listings in the game and try again.";
pub const NO_SNAPSHOT: &str = "Open Trade → Marketplace → My Listings in the game first.";
pub const STALE_SNAPSHOT: &str = "Open (or re-open) Trade → Marketplace → My Listings in the game first.";
pub const DEFAULT_SKIP_NOTE: &str = "no longer worth listing at today's prices";
pub const DIALOG_MAY_BE_OPEN: &str =
    " The \"Would you like to list the item?\" dialog may still be open: click No in the game.";
/// Epic, Legendary, Rare, Unique: what a crawl reads, in this order.
pub const CRAWL_RARITIES: [i32; 4] = [5, 6, 4, 7];
/// Arrow positions tried for View Market's next page (it shifts as the page counter widens).
pub const NEXT_PAGE_ATTEMPTS: i32 = 6;
pub const NEXT_PAGE_TIMEOUT_S: f64 = 1.5;
/// A crawl reports its progress every this many pages.
pub const CRAWL_PROGRESS_EVERY: u32 = 100;
/// All-roll result pages read per item (cheapest first).
pub const MARKET_PAGES: usize = 10;
/// Same-roll search result pages read per item.
pub const SAME_SEARCH_PAGES: usize = 5;
/// Listings per View Market page.
pub const MARKET_PAGE_SIZE: usize = 10;
/// Seconds to wait for the game's answer to a click (a listing, a transfer, a page of results).
pub const REGISTER_TIMEOUT_S: f64 = 5.0;
/// Seconds to wait for a new listing to show up in My Listings.
pub const CONFIRM_TIMEOUT_S: f64 = 3.0;

/// The rarity's name as the game shows it, or its number.
pub fn rarity_name(rarity: i32) -> String {
    let name = match rarity {
        1 => "Poor",
        2 => "Common",
        3 => "Uncommon",
        4 => "Rare",
        5 => "Epic",
        6 => "Legendary",
        7 => "Unique",
        8 => "Artifact",
        other => return other.to_string(),
    };
    name.to_string()
}

/// Called after each item's market search with `(item_id, started, all-roll rows, complete)`:
/// `started` is [`MarketplaceGame::now`] when the all-roll search was clicked, `complete` is true
/// when every listing of the item was seen. Lets the market history mark vanished listings sold.
pub type ScanObserver = dyn Fn(&str, f64, &[MarketRow], bool) + Send + Sync;
/// Called after a crawl read every page of a rarity, with `(rarity, started)` (`started` as in
/// [`ScanObserver`]); returns how many known listings of it are gone since the last full crawl.
pub type PassObserver = dyn Fn(i32, f64) -> Option<u64> + Send + Sync;

/// Clicks through the Marketplace for one job run (DnDTools' `MarketplaceRunner`).
pub struct MarketplaceRunner {
    driver: Arc<dyn GameInput>,
    layout: MarketplaceLayout,
    game: Arc<dyn MarketplaceGame>,
    tab_mapping: Vec<i32>,
    cancel: CancelToken,
    pause: Box<dyn Fn() + Send>,
    safety: Arc<dyn SafetyCheck>,
    register_timeout: f64,
    confirm_timeout: f64,
    scan_observer: Option<Arc<ScanObserver>>,
    pass_observer: Option<Arc<PassObserver>>,
    /// The My Listings page on screen (0-based), as the game last reported it.
    page: Cell<i64>,
    /// The next-page arrow position that worked last.
    arrow_attempt: Cell<i32>,
    last_click: Cell<Option<ScreenPoint>>,
}

impl MarketplaceRunner {
    /// A runner clicking through `layout` with `driver`, reading the game's answers from `game`.
    /// `tab_mapping` is the stash id behind each Marketplace tab icon after the inventory's
    /// (`tab_icon_index`); `pause` is the short wait after each click (it should return early once
    /// `cancel` is cancelled). No safety monitor until [`MarketplaceRunner::with_safety`].
    pub fn new(
        driver: Arc<dyn GameInput>,
        layout: MarketplaceLayout,
        game: Arc<dyn MarketplaceGame>,
        tab_mapping: Vec<i32>,
        cancel: CancelToken,
        pause: Box<dyn Fn() + Send>,
    ) -> Self {
        MarketplaceRunner {
            driver,
            layout,
            game,
            tab_mapping,
            cancel,
            pause,
            safety: Arc::new(NullSafety),
            register_timeout: REGISTER_TIMEOUT_S,
            confirm_timeout: CONFIRM_TIMEOUT_S,
            scan_observer: None,
            pass_observer: None,
            page: Cell::new(0),
            arrow_attempt: Cell::new(0),
            last_click: Cell::new(None),
        }
    }

    pub fn with_safety(self, safety: Arc<dyn SafetyCheck>) -> Self {
        MarketplaceRunner { safety, ..self }
    }

    /// Seconds to wait for the game's answers (defaults [`REGISTER_TIMEOUT_S`], [`CONFIRM_TIMEOUT_S`]).
    pub fn with_timeouts(self, register_timeout: f64, confirm_timeout: f64) -> Self {
        MarketplaceRunner { register_timeout, confirm_timeout, ..self }
    }

    pub fn with_scan_observer(self, scan_observer: Option<Arc<ScanObserver>>) -> Self {
        MarketplaceRunner { scan_observer, ..self }
    }

    pub fn with_pass_observer(self, pass_observer: Option<Arc<PassObserver>>) -> Self {
        MarketplaceRunner { pass_observer, ..self }
    }
}

// --- steps every flow shares ---------------------------------------------------------------------

impl MarketplaceRunner {
    fn point(&self, key: &str) -> ScreenPoint {
        self.layout.point(key)
    }

    fn check(&self) -> Step {
        check_cancel(&self.cancel, &*self.safety)
    }

    fn click(&self, point: ScreenPoint) -> Step {
        self.check()?;
        self.driver.click(point)?;
        self.last_click.set(Some(point));
        (self.pause)();
        Ok(())
    }

    fn settle(&self) {
        for _ in 0..SEARCH_SETTLE_PAUSES {
            (self.pause)();
        }
    }

    fn safety_checkpoint(&self) -> Step {
        safety_checkpoint(&*self.safety)
    }

    /// Stops when the cursor left the spot clicked last: someone took the mouse.
    fn check_mouse_still(&self) -> Step {
        match self.last_click.get() {
            Some(point) if cursor_moved_from(&*self.driver, point)? => Err(Halt::stop(MOUSE_MOVED)),
            _ => Ok(()),
        }
    }

    fn stale_message(&self, snapshot: Option<&ListingsSnapshot>) -> Option<&'static str> {
        match snapshot {
            None => Some(NO_SNAPSHOT),
            Some(s) if self.game.now() - s.received_at > MAX_SNAPSHOT_AGE_S => Some(STALE_SNAPSHOT),
            Some(_) => None,
        }
    }

    /// Refuses the locked Seasonal Shared Stash and unmapped tabs before anything is clicked.
    fn entries_message(&self, entries: &[PlanEntry]) -> Option<String> {
        let off_limits = entries.iter().find(|e| is_off_limits_stash(&e.stash_id));
        if let Some(entry) = off_limits {
            return Some(off_limits_message(&entry.name));
        }
        let unmapped = entries.iter().find(|e| tab_icon_index(&e.stash_id, &self.tab_mapping).is_none());
        unmapped.map(|entry| unmapped_message(&entry.name))
    }

    /// `None` when it is safe to begin, else why not (`_start`). Ends on a freshly confirmed My
    /// Listings, so [`MarketplaceGame::snapshot`] is the snapshot the game just sent.
    fn start(&self, entries: &[PlanEntry], need_spot: bool) -> Option<String> {
        let snapshot = self.game.snapshot();
        let refusal = self
            .entries_message(entries)
            .or_else(|| self.stale_message(snapshot.as_ref()).map(str::to_string))
            .or_else(|| {
                let full = snapshot.as_ref().is_some_and(|s| s.available.is_empty());
                (need_spot && full).then(|| NO_FREE_SPOTS.to_string())
            })
            .or_else(|| (!self.safety.checkpoint()).then(|| safety_stop_message(&*self.safety)));
        if refusal.is_some() {
            return refusal;
        }
        match self.verify_my_listings(true) {
            Ok(Some(_)) => None,
            Ok(None) => Some(NOT_ON_MY_LISTINGS.to_string()),
            Err(Halt::Stop { message, .. }) => Some(message),
            Err(Halt::Failed(message)) => Some(unexpected(&message)),
        }
    }

    /// Opens My Listings; the fresh snapshot the game answers with, or `None`.
    ///
    /// Clicking the tab of the screen already shown may not make the game re-send the list, so
    /// from My Listings itself View Market is opened first (`via_market`). The game reopens My
    /// Listings on the page last used, so the page it reports is tracked, not required to be 1.
    fn verify_my_listings(&self, via_market: bool) -> Step<Option<ListingsSnapshot>> {
        if via_market {
            self.click(self.point("view_market_tab"))?;
            self.settle();
        }
        let since = self.game.now();
        self.click(self.point("my_listings_tab"))?;
        match self.game.wait_for_fresh_snapshot(since, self.register_timeout) {
            Some(snapshot) if snapshot.current_page >= FIRST_PAGE => {
                self.page.set(snapshot.current_page - FIRST_PAGE);
                Ok(Some(snapshot))
            }
            _ => Ok(None),
        }
    }

    fn confirm_my_listings(&self, via_market: bool) -> Step<ListingsSnapshot> {
        self.verify_my_listings(via_market)?.ok_or_else(|| Halt::stop(NOT_ON_MY_LISTINGS))
    }

    fn go_to_spot(&self, order_index: i64) -> Step {
        let Ok(index) = i32::try_from(order_index) else {
            return Err(Halt::stop(NO_FREE_SPOTS));
        };
        let (page, row) = spot_location(index);
        if i64::from(page) >= MAX_PAGES {
            return Err(Halt::stop(NO_FREE_SPOTS));
        }
        self.show_page(i64::from(page))?;
        self.click(self.layout.spot_row(row))
    }

    /// Turns My Listings to `page` with the arrows, each turn confirmed by the game; the latest
    /// snapshot.
    fn show_page(&self, page: i64) -> Step<Option<ListingsSnapshot>> {
        let mut snapshot = self.game.snapshot();
        while self.page.get() != page {
            let step = if self.page.get() < page { 1 } else { -1 };
            let since = self.game.now();
            self.click(self.point(if step > 0 { "next_page_arrow" } else { "prev_page_arrow" }))?;
            snapshot = self.game.wait_for_fresh_snapshot(since, self.register_timeout);
            let expected = FIRST_PAGE + self.page.get() + step;
            if !matches!(&snapshot, Some(s) if s.current_page == expected) {
                let shown = self.page.get() + step + 1;
                return Err(Halt::stop(format!("Couldn't turn My Listings to page {shown}, so nothing was clicked there.")));
            }
            self.page.set(self.page.get() + step);
        }
        Ok(snapshot)
    }

    /// Opens the entry's stash tab and clicks the item (never in the locked seasonal stash).
    fn select_item(&self, entry: &PlanEntry) -> Step {
        if is_off_limits_stash(&entry.stash_id) {
            return Err(Halt::stop(off_limits_message(&entry.name)));
        }
        let Some(icon) = tab_icon_index(&entry.stash_id, &self.tab_mapping) else {
            return Err(Halt::stop(unmapped_message(&entry.name)));
        };
        self.click(self.layout.tab_icon(icon as i32))?;
        self.click(item_centre(&self.layout, entry)?)
    }
}

impl Runner for MarketplaceRunner {
    fn run(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult), reprice: Option<&dyn Reprice>) -> RunReport {
        self.list_entries(entries, dry_run, on_progress, reprice)
    }

    fn price_all(&self, entries: &[PlanEntry], on_progress: &mut dyn FnMut(ItemResult)) -> (HashMap<String, MarketBucket>, RunReport) {
        self.price_entries(entries, on_progress)
    }

    /// Crawls [`CRAWL_RARITIES`] with no class filter (see [`MarketplaceRunner::crawl_market_with`]).
    fn crawl_market(&self, pages: u32, on_progress: &mut dyn FnMut(ItemResult), is_old_page: Option<&OldPageCheck>) -> RunReport {
        self.crawl_market_with(pages, on_progress, &CRAWL_RARITIES, false, is_old_page)
    }

    fn collect_payouts(&self, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        self.collect(on_progress)
    }
}

/// Reports `result` both to the run's results and to its progress callback.
fn report_progress(results: &mut Vec<ItemResult>, on_progress: &mut dyn FnMut(ItemResult), result: ItemResult) {
    results.push(result.clone());
    on_progress(result);
}

/// The report of a run a [`Halt`] ended: a stop's own results are reported too (after `results`),
/// a failed input is described with `failed` (DnDTools' `run`/`price_all` said "Stopped: ...";
/// its other flows let the exception reach the job, which said "Unexpected error: ...").
fn halted(
    mut results: Vec<ItemResult>,
    halt: Halt,
    on_progress: &mut dyn FnMut(ItemResult),
    failed: fn(&str) -> String,
) -> RunReport {
    match halt {
        Halt::Stop { message, results: extra } => {
            for result in extra {
                report_progress(&mut results, on_progress, result);
            }
            RunReport { results, stopped_reason: Some(message) }
        }
        Halt::Failed(message) => RunReport { results, stopped_reason: Some(failed(&message)) },
    }
}

/// How `run` and `price_all` described an unexpected failure.
fn stopped(message: &str) -> String {
    format!("Stopped: {message}")
}
