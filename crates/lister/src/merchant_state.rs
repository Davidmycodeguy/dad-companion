//! Tracks merchant packets so the merchant seller can confirm where it is and what was sold.
//!
//! Port of DnDTools' `src/models/merchant_state.py`.

use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::clock::{MonotonicClock, SharedClock};

/// `SS2C_MERCHANT_STOCK_SELL_BACK_RES.result` for a sale that went through.
pub const SELL_SUCCESS: i64 = 1;
const QUEST_MARK: &str = "Id_Quest_";

/// `SS2C_MERCHANT_QUEST_LIST_INFO_RES`: arrives whenever a merchant's window opens. Only the raw
/// `questId` of each quest is needed; `requiredQuestMerchantId` names a prerequisite merchant, not
/// the owner, so DnDTools ignores it and this mirror leaves it out entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestListMessage {
    /// Raw `questId`s, e.g. `"QuestData:Id_Quest_TheCollector_01"`.
    pub quest_ids: Vec<String>,
}

/// `SS2C_MERCHANT_STOCK_SELL_BACK_RES`: the game's answer to Make Deal on the Sell tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SellBackMessage {
    pub result: i64,
    /// `merchantResult.deleteUniqueIds`: `itemUniqueId`s that left the stash, as the game's signed
    /// `int64` field (see [`SellBack::deleted_ids`] for why these can be negative).
    pub delete_unique_ids: Vec<i64>,
}

/// The game's answer to Make Deal on the Sell tab, after `received_at` and id decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct SellBack {
    pub received_at: f64,
    pub result: i64,
    /// `itemUniqueId`s that left the stash (were sold).
    pub deleted_ids: Vec<String>,
}

/// `deleteUniqueIds` is a signed `int64` while `itemUniqueId` is `uint64`: ids from `2**63` arrive
/// negative and must be read back as the unsigned id the game actually means.
fn unsigned_item_id(id: i64) -> String {
    (id as i128).rem_euclid(1i128 << 64).to_string()
}

/// `design_id.split(":")[-1]`: the bare id after the last `:`.
fn bare(design_id: &str) -> &str {
    design_id.rsplit(':').next().unwrap_or(design_id)
}

/// True when a merchant's quest list (already bare-ided, see [`MerchantState::handle_quest_list`])
/// names `key` (e.g. `"TheCollector"` matches `Id_Quest_TheCollector_01`).
fn names_merchant(bare_quest_ids: &[String], key: &str) -> bool {
    let prefix = format!("{QUEST_MARK}{key}_");
    bare_quest_ids.iter().any(|q| q.starts_with(&prefix))
}

#[derive(Default)]
struct Inner {
    quest_list: Option<(f64, Vec<String>)>,
    sell_back: Option<SellBack>,
}

/// Tracks merchant packets across threads, the same producer/consumer shape as
/// [`crate::marketplace_state::MarketplaceState`]: `handle_*` from the packet reader thread,
/// `wait_for_*` (blocking, with a timeout) from the merchant seller's thread.
pub struct MerchantState {
    clock: SharedClock,
    inner: Mutex<Inner>,
    cond: Condvar,
}

impl Default for MerchantState {
    fn default() -> Self {
        Self::new(MonotonicClock::shared())
    }
}

impl MerchantState {
    pub fn new(clock: SharedClock) -> Self {
        MerchantState { clock, inner: Mutex::new(Inner::default()), cond: Condvar::new() }
    }

    pub fn now(&self) -> f64 {
        self.clock.now()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("MerchantState mutex is never held across a panic")
    }

    pub fn handle_quest_list(&self, message: &QuestListMessage) {
        let quests: Vec<String> = message.quest_ids.iter().map(|q| bare(q).to_string()).collect();
        let mut inner = self.lock();
        inner.quest_list = Some((self.clock.now(), quests));
        self.cond.notify_all();
    }

    pub fn handle_sell_back(&self, message: &SellBackMessage) {
        let reply = SellBack {
            received_at: self.clock.now(),
            result: message.result,
            deleted_ids: message.delete_unique_ids.iter().map(|&id| unsigned_item_id(id)).collect(),
        };
        let mut inner = self.lock();
        inner.sell_back = Some(reply);
        self.cond.notify_all();
    }

    /// Waits (up to `timeout` seconds) for a quest list received after `since`, returning whether
    /// it names merchant `key`. A quest list not naming `key` is *not* re-waited on — mirrors the
    /// Python's single `wait_for` call, which reports on whichever quest list is fresh once the
    /// wait ends, not necessarily one for `key`.
    pub fn wait_for_merchant(&self, key: &str, since: f64, timeout: f64) -> bool {
        let guard = self.lock();
        let fresh = |inner: &Inner| matches!(&inner.quest_list, Some((t, _)) if *t > since);
        if fresh(&guard) {
            return names_merchant(&guard.quest_list.as_ref().expect("fresh() checked Some").1, key);
        }
        let (guard, _) = self
            .cond
            .wait_timeout_while(guard, Duration::from_secs_f64(timeout.max(0.0)), |inner| !fresh(inner))
            .expect("MerchantState mutex is never held across a panic");
        fresh(&guard) && names_merchant(&guard.quest_list.as_ref().expect("fresh() checked Some").1, key)
    }

    /// The first sell reply received after `since`, or `None` if the game did not answer in time.
    pub fn wait_for_sell_back(&self, since: f64, timeout: f64) -> Option<SellBack> {
        let guard = self.lock();
        let fresh = |inner: &Inner| matches!(&inner.sell_back, Some(r) if r.received_at > since);
        let guard = if fresh(&guard) {
            guard
        } else {
            self.cond
                .wait_timeout_while(guard, Duration::from_secs_f64(timeout.max(0.0)), |inner| !fresh(inner))
                .expect("MerchantState mutex is never held across a panic")
                .0
        };
        guard.sell_back.clone().filter(|r| r.received_at > since)
    }
}
