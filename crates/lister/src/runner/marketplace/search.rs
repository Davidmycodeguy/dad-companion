//! Reading the market: `price_all` (each entry's exact item, from its listing form) and
//! `crawl_market` (View Market, one rarity at a time), with the result paging they share.

use std::collections::{BTreeSet, HashMap};

use market::MarketRow;

use super::{
    halted, rarity_name, refused, report_progress, stopped, MarketplaceRunner, CLASS_COUNT, CRAWL_PROGRESS_EVERY,
    MARKET_PAGES, MARKET_PAGE_SIZE, NEXT_PAGE_ATTEMPTS, NEXT_PAGE_TIMEOUT_S, NO_FREE_SPOTS, SAME_SEARCH_PAGES,
};
use crate::job::{ItemResult, OldPageCheck, RunReport};
use crate::plan::{MarketBucket, PlanEntry};
use crate::runner::{unexpected, Halt, Step};

/// What paging through one search read (`_read_pages`' tuple).
struct PageRead {
    rows: Vec<MarketRow>,
    /// The results ran out: every listing was seen.
    complete: bool,
    /// A page never arrived, so the view is incomplete.
    failed: bool,
}

/// What a crawl of one rarity read.
struct CrawlRead {
    pages: u32,
    listings: usize,
    complete: bool,
}

impl MarketplaceRunner {
    /// Looks up each entry's current listings via the in-game Search flow. Never clicks Create
    /// Listing. A [`MarketBucket`] is `degraded` when a result page never arrived.
    pub(super) fn price_entries(
        &self,
        entries: &[PlanEntry],
        on_progress: &mut dyn FnMut(ItemResult),
    ) -> (HashMap<String, MarketBucket>, RunReport) {
        if let Some(refusal) = self.start(entries, true) {
            return (HashMap::new(), refused(refusal));
        }
        let Some(spot) = self.game.snapshot().and_then(|s| s.available.iter().copied().min()) else {
            return (HashMap::new(), refused(NO_FREE_SPOTS));
        };
        let mut rows_by_uid = HashMap::new();
        let mut results = Vec::new();
        for entry in entries {
            let found = match self.safety_checkpoint().and_then(|()| self.search_market(entry, spot)) {
                Ok(found) => found,
                Err(halt) => return (rows_by_uid, halted(results, halt, on_progress, stopped)),
            };
            let note = if found.degraded { " (search incomplete)" } else { "" };
            let status = if found.same.is_empty() && found.all.is_empty() { "no_results" } else { "priced" };
            let message = format!("{} with the same rolls, {} of any roll{note}", found.same.len(), found.all.len());
            rows_by_uid.insert(entry.unique_id.clone(), found);
            report_progress(&mut results, on_progress, ItemResult::new(&entry.unique_id, &entry.name, status).with_message(message));
            self.safety.snapshot_position();
        }
        (rows_by_uid, RunReport { results, stopped_reason: None })
    }

    /// Searches the market for `entry` from its listing form, then returns to a confirmed My
    /// Listings. Starts on My Listings (confirmed by `start` or the previous search / listing).
    pub(super) fn search_market(&self, entry: &PlanEntry, spot: i64) -> Step<MarketBucket> {
        self.go_to_spot(spot)?;
        self.select_item(entry)?;
        let since = self.game.now();
        // The game pre-fills the search with our item's random attributes: same-roll listings.
        self.click(self.point("form_search_button"))?;
        let same = self.read_pages(self.game.wait_for_item_list(since, self.register_timeout), SAME_SEARCH_PAGES)?;
        self.settle();
        // Then every roll of this item.
        self.click(self.point("market_attr_reset"))?;
        self.settle();
        let started = self.game.now();
        self.click(self.point("market_search_button"))?;
        let every = self.read_pages(self.game.wait_for_item_list(started, self.register_timeout), MARKET_PAGES)?;
        check_search_matches(entry, &same.rows, &every.rows)?;
        if let Some(observer) = &self.scan_observer {
            observer(&entry.item_id, started, &every.rows, every.complete);
        }
        // Via View Market: if the search never opened we are still on My Listings, and clicking
        // its own tab would not make the game resend the list.
        self.confirm_my_listings(true)?;
        Ok(MarketBucket { same: same.rows, all: every.rows, degraded: same.failed || every.failed })
    }

    /// Keeps paging (cheapest first) from `first_page` until the results end or `max_pages`.
    fn read_pages(&self, first_page: Option<Vec<MarketRow>>, max_pages: usize) -> Step<PageRead> {
        let Some(mut rows) = first_page else {
            return Ok(PageRead { rows: Vec::new(), complete: false, failed: true });
        };
        let (mut size, mut pages) = (rows.len(), 1);
        while size >= MARKET_PAGE_SIZE && pages < max_pages {
            if self.results_ended(true) {
                return Ok(PageRead { rows, complete: true, failed: false });
            }
            let Some(page) = self.next_page()? else {
                // No next page: the end (a full last page) or a failed turn.
                let ended = self.results_ended(false);
                return Ok(PageRead { rows, complete: ended, failed: !ended });
            };
            size = page.len();
            pages += 1;
            rows.extend(page);
        }
        Ok(PageRead { rows, complete: size < MARKET_PAGE_SIZE, failed: false })
    }

    /// Clicks the next-page arrow (its position depends on the page counter's width), trying the
    /// position that worked last first. No fixed pause: the game's reply is the wait between pages.
    fn next_page(&self) -> Step<Option<Vec<MarketRow>>> {
        let first = self.arrow_attempt.get();
        let attempts = std::iter::once(first).chain((0..NEXT_PAGE_ATTEMPTS).filter(|&a| a != first));
        for attempt in attempts {
            let since = self.game.now();
            self.check()?;
            let point = self.layout.next_page_candidate(attempt);
            self.driver.click(point)?;
            self.last_click.set(Some(point));
            if let Some(rows) = self.game.wait_for_item_list(since, NEXT_PAGE_TIMEOUT_S) {
                self.arrow_attempt.set(attempt);
                return Ok(Some(rows));
            }
        }
        Ok(None)
    }

    /// True when the game's page numbers say the page just read was the last one. The game may
    /// count pages from 0 or 1: before clicking on (`strict`), only "current == max" (the last
    /// page counting from 1) stops us; after a failed page turn, being on the last page by either
    /// count means the results simply ended.
    fn results_ended(&self, strict: bool) -> bool {
        match self.game.last_item_page() {
            Some((current, total)) if total > 0 => current >= if strict { total } else { total - 1 },
            _ => false,
        }
    }
}

impl MarketplaceRunner {
    /// Reads the newest listings of every item, one rarity at a time (View Market's filter).
    ///
    /// Nothing is bought or listed (the Buy buttons are never clicked); the captured pages feed the
    /// local market history. `gear_only` ticks every class so materials, potions and treasure drop
    /// out. `is_old_page(rows)` returning true stops a rarity early once the newest-first results
    /// reach listings already recorded (an incremental top-up).
    pub fn crawl_market_with(
        &self,
        pages: u32,
        on_progress: &mut dyn FnMut(ItemResult),
        rarities: &[i32],
        gear_only: bool,
        is_old_page: Option<&OldPageCheck>,
    ) -> RunReport {
        if let Some(refusal) = self.start(&[], false) {
            return refused(refusal);
        }
        let mut results = Vec::new();
        match self.crawl_rarities(pages, on_progress, rarities, gear_only, is_old_page, &mut results) {
            Ok(()) => RunReport { results, stopped_reason: None },
            Err(halt) => halted(results, halt, on_progress, unexpected),
        }
    }

    fn crawl_rarities(
        &self,
        pages: u32,
        on_progress: &mut dyn FnMut(ItemResult),
        rarities: &[i32],
        gear_only: bool,
        is_old_page: Option<&OldPageCheck>,
        results: &mut Vec<ItemResult>,
    ) -> Step {
        for &rarity in rarities {
            let started = self.game.now();
            let read = self.crawl_rarity(rarity, pages, gear_only, is_old_page, on_progress)?;
            let mut message = format!("{} pages, {} listings", read.pages, read.listings);
            // Only a crawl that saw every listing of the rarity can tell which ones are gone.
            let observer = self.pass_observer.as_ref().filter(|_| read.complete && is_old_page.is_none());
            if let Some(gone) = observer.and_then(|observer| observer(rarity, started)) {
                message.push_str(&format!("; {gone} gone since the last full crawl (likely sold)"));
            }
            let item = ItemResult::new(format!("rarity-{rarity}"), rarity_name(rarity), "crawled").with_message(message);
            report_progress(results, on_progress, item);
        }
        self.confirm_my_listings(false)?;
        Ok(())
    }

    fn crawl_rarity(
        &self,
        rarity: i32,
        pages: u32,
        gear_only: bool,
        is_old_page: Option<&OldPageCheck>,
        on_progress: &mut dyn FnMut(ItemResult),
    ) -> Step<CrawlRead> {
        self.click(self.point("view_market_tab"))?;
        self.settle();
        self.click(self.point("market_reset_filters"))?;
        self.settle();
        self.click(self.point("rarity_dropdown"))?;
        self.settle();
        self.click(self.layout.rarity_option(rarity))?;
        if gear_only {
            // Ticking a class closes the dropdown: it is reopened each time.
            for index in 0..CLASS_COUNT {
                self.click(self.point("class_dropdown"))?;
                self.click(self.layout.class_option(index))?;
            }
        }
        let since = self.game.now();
        self.click(self.point("market_search_button"))?;
        let mut rows = self.game.wait_for_item_list(since, self.register_timeout).unwrap_or_default();
        let mut read = CrawlRead { pages: 0, listings: 0, complete: false };
        while !rows.is_empty() {
            read.pages += 1;
            read.listings += rows.len();
            if read.pages.is_multiple_of(CRAWL_PROGRESS_EVERY) {
                let message = format!("{} pages, {} listings so far", read.pages, read.listings);
                let id = format!("rarity-{rarity}-{}", read.pages);
                on_progress(ItemResult::new(id, rarity_name(rarity), "crawling").with_message(message));
            }
            if rows.len() < MARKET_PAGE_SIZE || self.results_ended(true) {
                // The last page: everything was read.
                read.complete = true;
                break;
            }
            if read.pages >= pages || is_old_page.is_some_and(|is_old| is_old(&rows)) {
                break;
            }
            self.safety_checkpoint()?;
            self.check_mouse_still()?;
            let next = self.next_page()?;
            self.safety.snapshot_position();
            match next {
                Some(next) => rows = next,
                None => {
                    read.complete = self.results_ended(false);
                    break;
                }
            }
        }
        Ok(read)
    }
}

/// The form's Search looks up the *selected* item: other items mean we picked the wrong one.
fn check_search_matches(entry: &PlanEntry, same: &[MarketRow], every: &[MarketRow]) -> Step {
    if entry.item_id.is_empty() {
        return Ok(());
    }
    let others: BTreeSet<&str> =
        same.iter().chain(every).map(|r| r.item_id.as_str()).filter(|id| *id != entry.item_id).collect();
    match others.first() {
        None => Ok(()),
        Some(other) => Err(Halt::stop(format!(
            "The market search showed {other} instead of {}, so the wrong item may be selected (stash data out of \
             date or calibration off). Stopped before listing anything.",
            entry.name
        ))),
    }
}
