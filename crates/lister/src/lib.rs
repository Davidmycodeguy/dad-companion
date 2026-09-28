//! The auto market lister: build a reviewable pricing plan, track Marketplace/merchant packets,
//! and run one lister operation at a time on a background thread.
//!
//! Port of DnDTools' `src/market_lister.py`, `src/market_lister_job.py`,
//! `src/models/marketplace_state.py`, `src/models/merchant_seller.py`, `src/models/merchant_state.py`,
//! the pure logic of `src/market_service.py`, and the runners that click through the game
//! (`marketplace_runner.py`, `merchant_runner.py`, `marketplace_input.py`) in [`runner`], which
//! implement [`job::Runner`], [`job::MerchantRunner`] and [`job::Safety`].

pub mod clock;
pub mod job;
pub mod market_service;
pub mod marketplace_state;
pub mod merchant_seller;
pub mod merchant_state;
pub mod plan;
pub mod runner;

pub use clock::{Clock, FakeClock, MonotonicClock, SharedClock};
