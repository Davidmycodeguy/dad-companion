//! Sells items to a merchant: stage them in the Sell box, Make Deal, confirm from the game's reply.
//! Port of DnDTools' `src/models/merchant_runner.py`.

use std::cell::Cell;
use std::sync::Arc;

use input::marketplace::{tab_icon_index, MarketplaceLayout};

use super::{
    check_cancel, cursor_moved_from, friendly_reason, is_off_limits_stash, item_centre, off_limits_message,
    reason_of, safety_checkpoint, unexpected, unmapped_message, GameInput, Halt, MerchantGame, NullSafety,
    SafetyCheck, ScreenPoint, Step, MOUSE_MOVED, OFF_LIMITS_REASON,
};
use crate::job::{CancelToken, ItemResult, MerchantRunner, RunReport};
use crate::merchant_seller::{merchant_value, pack_sell_box, sale_outcome, Placement, SELL_BOX_COLUMNS, SELL_BOX_ROWS};
use crate::merchant_state::SELL_SUCCESS;
use crate::plan::{PlanEntry, UNMAPPED_TAB_REASON};

/// `Id_Merchant_TheCollector`: he took every kind of loot tried in game.
pub const MERCHANT_KEY: &str = "TheCollector";
pub const MERCHANT_NAME: &str = "The Collector";
/// Merchants & Travelers: Alchemist, Tavern Master, The Collector, ...
pub const MERCHANT_CARD_INDEX: i32 = 2;
pub const MERCHANT_OPEN_TIMEOUT_S: f64 = 5.0;
pub const SELL_REPLY_TIMEOUT_S: f64 = 6.0;
pub const NOTHING_TO_SELL: &str = "Nothing to sell.";
pub const TOO_BIG: &str = "too big for the sell box";
pub const NOT_AT_MERCHANT: &str = "Couldn't open The Collector — show the lobby in the game (close any merchant, \
                                   Marketplace or menu window) and try again.";
pub const NO_DEAL_REPLY: &str =
    "The game didn't confirm the sale — check The Collector's Buyback tab before trying again.";
pub const ITEMS_LEFT_STAGED: &str =
    " Nothing in the sell box was sold — press Escape in the game to put the items back.";
pub const NOTHING_SELLABLE: &str = "Nothing could be sold — see each item for why.";
pub const NOTHING_SOLD: &str =
    "The game answered Make Deal but sold none of the picked items — check The Collector's Sell box.";
pub const DRY_RUN_FIRST_BOX: &str = "not staged — a dry run fills only the first Sell box";
pub const DRY_RUN_CANCELLED: &str = "Cancelled — the dry run's items were put back.";

/// Sells plan entries to [`MERCHANT_NAME`] for one job run (DnDTools' `MerchantRunner`).
pub struct MerchantSaleRunner {
    driver: Arc<dyn GameInput>,
    layout: MarketplaceLayout,
    game: Arc<dyn MerchantGame>,
    tab_mapping: Vec<i32>,
    cancel: CancelToken,
    pause: Box<dyn Fn() + Send>,
    safety: Arc<dyn SafetyCheck>,
    open_timeout: f64,
    reply_timeout: f64,
    last_point: Cell<Option<ScreenPoint>>,
    /// The stash tab icon open in the merchant window, once known.
    open_tab: Cell<Option<usize>>,
}

impl MerchantSaleRunner {
    /// A runner clicking through `layout` with `driver`, reading the game's answers from `game`
    /// (see [`super::MarketplaceRunner::new`] for `tab_mapping` and `pause`). No safety monitor
    /// until [`MerchantSaleRunner::with_safety`].
    pub fn new(
        driver: Arc<dyn GameInput>,
        layout: MarketplaceLayout,
        game: Arc<dyn MerchantGame>,
        tab_mapping: Vec<i32>,
        cancel: CancelToken,
        pause: Box<dyn Fn() + Send>,
    ) -> Self {
        MerchantSaleRunner {
            driver,
            layout,
            game,
            tab_mapping,
            cancel,
            pause,
            safety: Arc::new(NullSafety),
            open_timeout: MERCHANT_OPEN_TIMEOUT_S,
            reply_timeout: SELL_REPLY_TIMEOUT_S,
            last_point: Cell::new(None),
            open_tab: Cell::new(None),
        }
    }

    pub fn with_safety(self, safety: Arc<dyn SafetyCheck>) -> Self {
        MerchantSaleRunner { safety, ..self }
    }

    /// Seconds to wait for the merchant window and for the answer to Make Deal (defaults
    /// [`MERCHANT_OPEN_TIMEOUT_S`], [`SELL_REPLY_TIMEOUT_S`]).
    pub fn with_timeouts(self, open_timeout: f64, reply_timeout: f64) -> Self {
        MerchantSaleRunner { open_timeout, reply_timeout, ..self }
    }
}

/// Collects a sale's results, reporting each to the progress callback as it is recorded.
struct Recorder<'a> {
    results: Vec<ItemResult>,
    on_progress: &'a mut dyn FnMut(ItemResult),
}

impl Recorder<'_> {
    fn record(&mut self, result: ItemResult) {
        self.results.push(result.clone());
        (self.on_progress)(result);
    }

    fn finish(self, stopped_reason: Option<String>) -> RunReport {
        RunReport { results: self.results, stopped_reason }
    }
}

fn result(entry: &PlanEntry, status: &str, message: impl Into<String>) -> ItemResult {
    ItemResult::new(&entry.unique_id, &entry.name, status).with_message(message)
}

impl MerchantRunner for MerchantSaleRunner {
    /// Sells `entries` to the merchant; a dry run stages the first Sell box, then puts it back.
    fn sell(&self, entries: &[PlanEntry], dry_run: bool, on_progress: &mut dyn FnMut(ItemResult)) -> RunReport {
        if entries.is_empty() {
            return RunReport { results: Vec::new(), stopped_reason: Some(NOTHING_TO_SELL.to_string()) };
        }
        let mut recorder = Recorder { results: Vec::new(), on_progress };
        let batches = self.batches(entries, &mut recorder);
        if batches.is_empty() {
            return recorder.finish(Some(NOTHING_SELLABLE.to_string()));
        }
        match self.sell_batches(&batches, dry_run, &mut recorder) {
            Ok(()) => recorder.finish(None),
            Err(Halt::Stop { message, .. }) => recorder.finish(Some(message)),
            // DnDTools let the exception reach the job, which said so.
            Err(Halt::Failed(message)) => recorder.finish(Some(unexpected(&message))),
        }
    }
}

impl MerchantSaleRunner {
    fn sell_batches(&self, batches: &[Vec<Placement>], dry_run: bool, recorder: &mut Recorder<'_>) -> Step {
        self.safety_checkpoint()?;
        self.open_merchant()?;
        self.click(self.layout.point("merchant_sell_tab"))?;
        // Make Deal sells, never buys back.
        self.click(self.layout.point("merchant_sell_mode"))?;
        if dry_run {
            return self.dry_run(batches, recorder);
        }
        for batch in batches {
            self.stage(batch)?;
            self.deal(batch, recorder)?;
        }
        self.leave(false)
    }

    /// Stages the first Sell box, reports every item, then puts the items back.
    fn dry_run(&self, batches: &[Vec<Placement>], recorder: &mut Recorder<'_>) -> Step {
        let Some((first, rest)) = batches.split_first() else {
            return Err(Halt::stop(NOTHING_SELLABLE));
        };
        self.stage(first)?;
        for placement in first {
            let entry = &placement.entry;
            let note = format!("staged — {MERCHANT_NAME} would pay {}g", merchant_value(entry));
            recorder.record(result(entry, "dry_run", note));
        }
        for placement in rest.iter().flatten() {
            recorder.record(result(&placement.entry, "skipped", DRY_RUN_FIRST_BOX));
        }
        self.leave(true)?;
        if self.cancel.is_cancelled() {
            return Err(Halt::stop(DRY_RUN_CANCELLED));
        }
        Ok(())
    }

    /// Sell-box batches for the entries that can be sold; the rest are reported as failed.
    fn batches(&self, entries: &[PlanEntry], recorder: &mut Recorder<'_>) -> Vec<Vec<Placement>> {
        let mut sellable = Vec::new();
        for entry in entries {
            if is_off_limits_stash(&entry.stash_id) {
                recorder.record(result(entry, "failed", OFF_LIMITS_REASON));
            } else if tab_icon_index(&entry.stash_id, &self.tab_mapping).is_none() {
                recorder.record(result(entry, "failed", UNMAPPED_TAB_REASON));
            } else {
                sellable.push(entry.clone());
            }
        }
        let (batches, too_big) = pack_sell_box(sellable, SELL_BOX_COLUMNS, SELL_BOX_ROWS);
        for entry in &too_big {
            recorder.record(result(entry, "failed", TOO_BIG));
        }
        batches
    }

    fn open_merchant(&self) -> Step {
        let since = self.game.now();
        self.click(self.layout.point("merchants_tab"))?;
        self.click(self.layout.merchant_card(MERCHANT_CARD_INDEX))?;
        if !self.game.wait_for_merchant(MERCHANT_KEY, since, self.open_timeout) {
            return Err(Halt::stop(NOT_AT_MERCHANT));
        }
        // The window opens on whichever tab the game remembers.
        self.open_tab.set(None);
        Ok(())
    }

    fn stage(&self, batch: &[Placement]) -> Step {
        let mut dragged = 0;
        self.stage_each(batch, &mut dragged)
            .map_err(|halt| if dragged > 0 { halt.suffixed(ITEMS_LEFT_STAGED) } else { halt })
    }

    fn stage_each(&self, batch: &[Placement], dragged: &mut usize) -> Step {
        for (stash_id, placements) in by_tab(batch) {
            let name = &placements[0].entry.name;
            if is_off_limits_stash(stash_id) {
                return Err(Halt::stop(off_limits_message(name)));
            }
            let icon = tab_icon_index(stash_id, &self.tab_mapping).ok_or_else(|| Halt::stop(unmapped_message(name)))?;
            if Some(icon) != self.open_tab.get() {
                self.click(self.layout.tab_icon(icon as i32))?;
                self.open_tab.set(Some(icon));
            }
            for placement in placements {
                let entry = &placement.entry;
                let target = sell_box_centre(&self.layout, placement)?;
                self.drag(item_centre(&self.layout, entry)?, target)?;
                *dragged += 1;
            }
        }
        Ok(())
    }
}

impl MerchantSaleRunner {
    /// Clicks Make Deal for one staged batch and records what the game says was sold.
    fn deal(&self, batch: &[Placement], recorder: &mut Recorder<'_>) -> Step {
        let entries: Vec<PlanEntry> = batch.iter().map(|placement| placement.entry.clone()).collect();
        self.check()
            .and_then(|()| self.safety_checkpoint())
            .and_then(|()| self.check_mouse_still())
            .map_err(|halt| halt.suffixed(ITEMS_LEFT_STAGED))?;
        let since = self.game.now();
        self.click(self.layout.point("merchant_make_deal"))?;
        let Some(reply) = self.game.wait_for_sell_back(since, self.reply_timeout) else {
            return Err(Halt::stop(NO_DEAL_REPLY));
        };
        if reply.result != SELL_SUCCESS {
            let code = reply.result;
            return Err(Halt::stop(format!("{MERCHANT_NAME} refused the deal (game code {code}).{ITEMS_LEFT_STAGED}")));
        }
        let outcome = sale_outcome(&entries, &reply.deleted_ids);
        if outcome.sold.is_empty() && outcome.unexpected.is_empty() {
            return Err(Halt::stop(format!("{NOTHING_SOLD}{ITEMS_LEFT_STAGED}")));
        }
        for entry in &outcome.sold {
            recorder.record(result(entry, "sold", format!("{}g", merchant_value(entry))));
        }
        for entry in &outcome.not_taken {
            recorder.record(result(entry, "not_taken", format!("{MERCHANT_NAME} didn't take it — it's still in your stash")));
        }
        if !outcome.unexpected.is_empty() {
            let ids = outcome.unexpected.join(", ");
            return Err(Halt::stop(format!(
                "Sold an item that wasn't picked (id {ids}) — buy it back from {MERCHANT_NAME}'s Buyback tab now."
            )));
        }
        Ok(())
    }

    /// Escapes back to the merchant grid (staged items go back to the stash) — but never after a
    /// safety stop, when the key could land in another window.
    fn leave(&self, staged: bool) -> Step {
        let safe = self.safety_checkpoint().and_then(|()| match reason_of(&*self.safety) {
            Some(reason) if self.cancel.is_cancelled() => {
                Err(Halt::stop(format!("Stopped for safety: {}", friendly_reason(&reason))))
            }
            _ => Ok(()),
        });
        safe.map_err(|halt| if staged { halt.suffixed(&format!(".{ITEMS_LEFT_STAGED}")) } else { halt })?;
        self.driver.press_escape()?;
        (self.pause)();
        Ok(())
    }

    fn check(&self) -> Step {
        check_cancel(&self.cancel, &*self.safety)
    }

    fn safety_checkpoint(&self) -> Step {
        safety_checkpoint(&*self.safety)
    }

    /// Stops when the cursor left the spot used last: someone took the mouse.
    fn check_mouse_still(&self) -> Step {
        match self.last_point.get() {
            Some(point) if cursor_moved_from(&*self.driver, point)? => Err(Halt::stop(MOUSE_MOVED)),
            _ => Ok(()),
        }
    }

    fn click(&self, point: ScreenPoint) -> Step {
        self.check()?;
        self.driver.click(point)?;
        self.last_point.set(Some(point));
        self.safety.snapshot_position();
        (self.pause)();
        Ok(())
    }

    fn drag(&self, source: ScreenPoint, target: ScreenPoint) -> Step {
        self.check()?;
        self.safety_checkpoint()?;
        self.check_mouse_still()?;
        self.driver.drag(source, target)?;
        self.last_point.set(Some(target));
        self.safety.snapshot_position();
        (self.pause)();
        Ok(())
    }
}

/// Centre of the Sell box cells `placement` takes.
fn sell_box_centre(layout: &MarketplaceLayout, placement: &Placement) -> Step<ScreenPoint> {
    let entry = &placement.entry;
    let cells = (i32::try_from(placement.col), i32::try_from(placement.row), i32::try_from(entry.width), i32::try_from(entry.height));
    let (Ok(col), Ok(row), Ok(width), Ok(height)) = cells else {
        return Err(Halt::stop(format!("{} has an impossible size, so it was not touched.", entry.name)));
    };
    Ok(layout.sell_box_centre(col, row, width, height))
}

/// Placements grouped by stash tab, tabs in order of first use.
fn by_tab(batch: &[Placement]) -> Vec<(&str, Vec<&Placement>)> {
    let mut groups: Vec<(&str, Vec<&Placement>)> = Vec::new();
    for placement in batch {
        let stash_id = placement.entry.stash_id.as_str();
        match groups.iter_mut().find(|(id, _)| *id == stash_id) {
            Some((_, placements)) => placements.push(placement),
            None => groups.push((stash_id, vec![placement])),
        }
    }
    groups
}
