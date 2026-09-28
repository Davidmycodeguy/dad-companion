//! The game's answers a runner waits for, read from its packets.
//!
//! Traits over [`MarketplaceState`] and [`MerchantState`] (the methods DnDTools' runners call on
//! them), so tests can script the game's answers instead of feeding packets.

use market::MarketRow;

use crate::marketplace_state::{ListingsSnapshot, MarketplaceState, RegisterOutcome};
use crate::merchant_state::{MerchantState, SellBack};

/// What the Marketplace runner needs from [`MarketplaceState`]. Times are that state's monotonic
/// seconds ([`MarketplaceGame::now`]); timeouts are seconds.
pub trait MarketplaceGame: Send + Sync {
    fn now(&self) -> f64;
    /// The latest My Listings snapshot, however old.
    fn snapshot(&self) -> Option<ListingsSnapshot>;
    /// A My Listings snapshot received after `since`, or `None` if none arrived in time.
    fn wait_for_fresh_snapshot(&self, since: f64, timeout: f64) -> Option<ListingsSnapshot>;
    /// The rows of the first search result page received after `since`.
    fn wait_for_item_list(&self, since: f64, timeout: f64) -> Option<Vec<MarketRow>>;
    /// `(currentPage, maxPage)` of the latest search result page.
    fn last_item_page(&self) -> Option<(i64, i64)>;
    /// Forgets the last Transfer All Items answer, before asking for a new one.
    fn begin_transfer(&self);
    /// The Transfer All Items result code, or `None` if the game did not answer in time.
    fn wait_for_transfer(&self, timeout: f64) -> Option<i64>;
    /// Forgets the last listing answer, before listing again.
    fn begin_register(&self);
    fn wait_for_register(&self, timeout: f64) -> RegisterOutcome;
    /// True once My Listings shows `unique_id` in a snapshot received after `since`.
    fn wait_for_listing(&self, unique_id: &str, since: f64, timeout: f64) -> bool;
}

impl MarketplaceGame for MarketplaceState {
    fn now(&self) -> f64 {
        MarketplaceState::now(self)
    }

    fn snapshot(&self) -> Option<ListingsSnapshot> {
        MarketplaceState::snapshot(self)
    }

    fn wait_for_fresh_snapshot(&self, since: f64, timeout: f64) -> Option<ListingsSnapshot> {
        MarketplaceState::wait_for_fresh_snapshot(self, since, timeout)
    }

    fn wait_for_item_list(&self, since: f64, timeout: f64) -> Option<Vec<MarketRow>> {
        MarketplaceState::wait_for_item_list(self, since, timeout)
    }

    fn last_item_page(&self) -> Option<(i64, i64)> {
        MarketplaceState::last_item_page(self)
    }

    fn begin_transfer(&self) {
        MarketplaceState::begin_transfer(self);
    }

    fn wait_for_transfer(&self, timeout: f64) -> Option<i64> {
        MarketplaceState::wait_for_transfer(self, timeout)
    }

    fn begin_register(&self) {
        MarketplaceState::begin_register(self);
    }

    fn wait_for_register(&self, timeout: f64) -> RegisterOutcome {
        MarketplaceState::wait_for_register(self, timeout)
    }

    fn wait_for_listing(&self, unique_id: &str, since: f64, timeout: f64) -> bool {
        MarketplaceState::wait_for_listing(self, unique_id, since, timeout)
    }
}

/// What the merchant runner needs from [`MerchantState`].
pub trait MerchantGame: Send + Sync {
    fn now(&self) -> f64;
    /// True when a merchant window opened after `since` and it is merchant `key`'s.
    fn wait_for_merchant(&self, key: &str, since: f64, timeout: f64) -> bool;
    /// The first answer to Make Deal received after `since`, or `None` if none arrived in time.
    fn wait_for_sell_back(&self, since: f64, timeout: f64) -> Option<SellBack>;
}

impl MerchantGame for MerchantState {
    fn now(&self) -> f64 {
        MerchantState::now(self)
    }

    fn wait_for_merchant(&self, key: &str, since: f64, timeout: f64) -> bool {
        MerchantState::wait_for_merchant(self, key, since, timeout)
    }

    fn wait_for_sell_back(&self, since: f64, timeout: f64) -> Option<SellBack> {
        MerchantState::wait_for_sell_back(self, since, timeout)
    }
}
