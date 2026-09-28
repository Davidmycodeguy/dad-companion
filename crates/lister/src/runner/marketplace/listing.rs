//! `run`: lists plan entries one at a time (DnDTools' `MarketplaceRunner.run` and its helpers).

use std::borrow::Cow;

use super::{
    refused, report_progress, stopped, MarketplaceRunner, DEFAULT_SKIP_NOTE, DIALOG_MAY_BE_OPEN, NOT_ON_MY_LISTINGS,
    NO_FREE_SPOTS,
};
use crate::job::{ItemResult, Reprice, RunReport};
use crate::marketplace_state::{describe_fail_code, RegisterOutcome, FIRST_PAGE, ITEM_LEVEL_FAIL_CODES};
use crate::plan::PlanEntry;
use crate::runner::{cursor_moved_from, Halt, Step, UNFOCUSED_REASON};

const NO_RESPONSE_NOTE: &str = "no response — may be listed, fee may have been charged";
const NOT_CONFIRMED: &str = "Listing not confirmed by the game — check the Marketplace.";
const NOT_SEEN_NOTE: &str = "not seen in My Listings — fee may have been charged";
const MOVED_BEFORE_CREATE_NOTE: &str = "stopped before Create Listing — no fee charged";
const MOVED_BEFORE_CREATE: &str = "Stopped for safety: mouse moved during listing";

fn result(entry: &PlanEntry, status: &str, message: impl Into<String>) -> ItemResult {
    ItemResult::new(&entry.unique_id, &entry.name, status).with_message(message)
}

impl MarketplaceRunner {
    /// Lists each entry on the game's free spots, in order. `reprice(entry, market)` re-checks the
    /// market right before listing: a lower price is used if the market dropped (never a higher
    /// one), no price skips the item.
    pub(super) fn list_entries(
        &self,
        entries: &[PlanEntry],
        dry_run: bool,
        on_progress: &mut dyn FnMut(ItemResult),
        reprice: Option<&dyn Reprice>,
    ) -> RunReport {
        if let Some(refusal) = self.start(entries, true) {
            return refused(refusal);
        }
        // The fresh snapshot `start` waited for; its free spots (verified in game) are used in order.
        let Some(snapshot) = self.game.snapshot() else {
            return refused(NOT_ON_MY_LISTINGS);
        };
        let mut free_spots = snapshot.available;
        free_spots.sort_unstable();
        let mut consumed = 0;
        let mut results = Vec::new();
        for entry in entries {
            let outcome = match free_spots.get(consumed) {
                Some(&spot) => self.list_one(entry, spot, dry_run, reprice),
                None => Err(Halt::stop(NO_FREE_SPOTS)),
            };
            match outcome {
                Ok(item) => {
                    if item.status == "listed" || item.status == "dry_run" {
                        consumed += 1;
                    }
                    report_progress(&mut results, on_progress, item);
                }
                Err(Halt::Stop { message, results: extra }) => {
                    for item in extra {
                        report_progress(&mut results, on_progress, item);
                    }
                    return RunReport { results, stopped_reason: Some(message) };
                }
                // An unexpected failure is pinned on the entry being listed, keeping what was
                // already listed.
                Err(Halt::Failed(message)) => {
                    report_progress(&mut results, on_progress, result(entry, "failed", &message));
                    return RunReport { results, stopped_reason: Some(stopped(&message)) };
                }
            }
        }
        RunReport { results, stopped_reason: None }
    }

    fn list_one(&self, entry: &PlanEntry, spot: i64, dry_run: bool, reprice: Option<&dyn Reprice>) -> Step<ItemResult> {
        self.safety_checkpoint()?;
        let (entry, note) = match reprice {
            None => (Cow::Borrowed(entry), String::new()),
            Some(reprice) => {
                let decision = reprice.reprice(entry, &self.search_market(entry, spot)?);
                let Some(price) = decision.price else {
                    self.safety.snapshot_position();
                    let note = if decision.note.is_empty() { DEFAULT_SKIP_NOTE } else { &decision.note };
                    return Ok(result(entry, "skipped", note));
                };
                if price < entry.price {
                    let lowered = PlanEntry { price, fee: market::listing_fee(price), ..entry.clone() };
                    (Cow::Owned(lowered), format!(" (market moved: planned {}g)", entry.price))
                } else if !decision.note.is_empty() {
                    (Cow::Borrowed(entry), format!(" ({})", decision.note))
                } else {
                    (Cow::Borrowed(entry), String::new())
                }
            }
        };
        self.go_to_spot(spot)?;
        self.fill_form(&entry)?;
        let item = if dry_run {
            result(&entry, "dry_run", format!("would list at {}g{note}", entry.price))
        } else {
            let item = self.submit(&entry)?;
            if !note.is_empty() && item.status == "listed" {
                let message = format!("{}{note}", item.message);
                item.with_message(message)
            } else {
                item
            }
        };
        // Taken after the last click, so the next checkpoint only sees the player's movement.
        self.safety.snapshot_position();
        Ok(item)
    }

    fn fill_form(&self, entry: &PlanEntry) -> Step {
        self.select_item(entry)?;
        if entry.quantity > 1 {
            // Stacks: the whole stack is listed.
            self.click(self.point("quantity_field"))?;
            self.check()?;
            self.driver.clear_and_type(&entry.quantity.to_string())?;
            (self.pause)();
        }
        self.click(self.point("price_field"))?;
        self.check()?;
        self.driver.clear_and_type(&entry.price.to_string())?;
        (self.pause)();
        Ok(())
    }

    /// Stops before Create Listing when the cursor left the price box: the player took the mouse.
    fn check_cursor(&self, entry: &PlanEntry) -> Step {
        if cursor_moved_from(&*self.driver, self.point("price_field"))? {
            return Err(Halt::stop_with(result(entry, "failed", MOVED_BEFORE_CREATE_NOTE), MOVED_BEFORE_CREATE));
        }
        Ok(())
    }

    /// Clicks No on the confirmation dialog; `false` when another window is in front (no blind
    /// click).
    fn dismiss_listing_dialog(&self) -> Step<bool> {
        if self.safety.reason().as_deref() == Some(UNFOCUSED_REASON) {
            return Ok(false);
        }
        self.driver.click(self.point("confirm_listing_no"))?;
        (self.pause)();
        Ok(true)
    }

    /// Clicks Create Listing, answers the game's "Would you like to list the item?" with Yes, and
    /// confirms the listing from the game's replies. The fee is only charged on Yes, so a cancel
    /// arriving before it dismisses the dialog instead.
    fn submit(&self, entry: &PlanEntry) -> Step<ItemResult> {
        self.check_cursor(entry)?;
        let before = self.game.snapshot();
        self.game.begin_register();
        let since = self.game.now();
        self.check()?;
        self.driver.click(self.point("create_listing_button"))?;
        (self.pause)();
        if self.cancel.is_cancelled() {
            // Never leave the dialog open for a stray click to confirm.
            let dismissed = self.dismiss_listing_dialog()?;
            let suffix = if dismissed { "" } else { DIALOG_MAY_BE_OPEN };
            self.check().map_err(|stop| stop.suffixed(suffix))?;
        }
        self.driver.click(self.point("confirm_listing_yes"))?;
        match self.game.wait_for_register(self.register_timeout) {
            RegisterOutcome::Ok => {}
            RegisterOutcome::Timeout => {
                // In case the Yes click was lost and the dialog is still up.
                self.dismiss_listing_dialog()?;
                let fail = result(entry, "unconfirmed", NO_RESPONSE_NOTE);
                return Err(Halt::stop_with(fail, format!("{NOT_CONFIRMED}{DIALOG_MAY_BE_OPEN}")));
            }
            RegisterOutcome::Failed(code) => {
                let message = describe_fail_code(code);
                let fail = result(entry, "failed", &message);
                if !ITEM_LEVEL_FAIL_CODES.contains(&code) {
                    return Err(Halt::stop_with(fail, message));
                }
                // Only this item can't be listed. The form (or an error popup) may still be up:
                // only go on from a confirmed My Listings.
                if self.verify_my_listings(true)?.is_none() {
                    return Err(Halt::stop_with(fail, NOT_ON_MY_LISTINGS));
                }
                return Ok(fail);
            }
        }
        if !self.game.wait_for_listing(&entry.unique_id, since, self.confirm_timeout) {
            let fail = result(entry, "unconfirmed", NOT_SEEN_NOTE);
            let message =
                format!("Listed something but couldn't confirm it was {} — check My Listings and recalibrate.", entry.name);
            return Err(Halt::stop_with(fail, message));
        }
        let snapshot = self.game.snapshot();
        if let Some(shown) = snapshot.as_ref().filter(|_| snapshot != before) {
            // The game re-sent My Listings: it shows this page now.
            self.page.set((shown.current_page - FIRST_PAGE).max(0));
        }
        (self.pause)();
        Ok(result(entry, "listed", format!("{}g", entry.price)))
    }
}
