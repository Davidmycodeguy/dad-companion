//! `collect_payouts`: collects gold from sold listings and takes back expired items with
//! "Transfer All Items" (DnDTools' `MarketplaceRunner.collect_payouts`).

use std::collections::HashSet;
use std::ops::Range;

use input::marketplace::SPOTS_PER_PAGE;

use super::{halted, refused, report_progress, MarketplaceRunner, MAX_PAGES, MAX_PAYOUTS_PER_RUN, NOT_ON_MY_LISTINGS};
use crate::job::{ItemResult, RunReport};
use crate::marketplace_state::{describe_fail_code, ListingsSnapshot, MY_ITEM_SOLD, REGISTER_SUCCESS};
use crate::runner::{unexpected, Halt, Step};

/// My Listings pages that hold listings. Listings fill spots from the first one (they shift up when
/// one is collected), so every spot before the first free one is taken.
pub fn listing_pages(snapshot: &ListingsSnapshot) -> Range<i64> {
    let per_page = i64::from(SPOTS_PER_PAGE);
    let used = snapshot.available.iter().copied().min().unwrap_or(MAX_PAGES * per_page);
    let pages = if used > 0 { (used + per_page - 1) / per_page } else { 0 };
    0..pages
}

impl MarketplaceRunner {
    /// Collects every payout waiting in My Listings. The game destroys uncollected payouts after
    /// 7 days. Listings shift up after each transfer, so My Listings is re-opened (and confirmed)
    /// for fresh positions every time; the game only reports the page on screen, so every page
    /// holding listings is visited.
    pub(super) fn collect(&self, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        if let Some(refusal) = self.start(&[], false) {
            return refused(refusal);
        }
        let mut results = Vec::new();
        match self.collect_all(on_progress, &mut results) {
            Ok(()) => RunReport { results, stopped_reason: None },
            Err(halt) => halted(results, halt, on_progress, unexpected),
        }
    }

    fn collect_all(&self, on_progress: &mut dyn FnMut(ItemResult), results: &mut Vec<ItemResult>) -> Step {
        // The fresh snapshot `start` waited for.
        let mut snapshot = self.game.snapshot().ok_or_else(|| Halt::stop(NOT_ON_MY_LISTINGS))?;
        let mut visited = HashSet::new();
        for _ in 0..MAX_PAYOUTS_PER_RUN + MAX_PAGES as usize {
            visited.insert(self.page.get());
            let Some((order_index, state, item_id, price)) = snapshot.payouts.iter().min().cloned() else {
                let Some(page) = listing_pages(&snapshot).find(|page| !visited.contains(page)) else {
                    break;
                };
                snapshot = self.show_page(page)?.ok_or_else(|| Halt::stop(NOT_ON_MY_LISTINGS))?;
                continue;
            };
            self.go_to_spot(order_index)?;
            self.game.begin_transfer();
            self.click(self.point("transfer_all_button"))?;
            self.safety.snapshot_position();
            let code = self.game.wait_for_transfer(self.register_timeout);
            if code != Some(REGISTER_SUCCESS) {
                let message = code.map_or_else(|| "no response from the game".to_string(), describe_fail_code);
                let fail = ItemResult::new(order_index.to_string(), &item_id, "failed").with_message(&message);
                return Err(Halt::stop_with(fail, format!("Couldn't collect {item_id}: {message}")));
            }
            let note = if state == MY_ITEM_SOLD { format!("{price}g collected") } else { "expired item returned".to_string() };
            report_progress(results, on_progress, ItemResult::new(order_index.to_string(), &item_id, "collected").with_message(note));
            self.safety_checkpoint()?;
            self.check_mouse_still()?;
            snapshot = self.confirm_my_listings(true)?;
        }
        Ok(())
    }
}
