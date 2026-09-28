//! An injectable source of monotonic seconds.
//!
//! DnDTools' `MarketplaceState` and `MerchantState` both take a `clock=time.monotonic` parameter
//! so their tests can control time without sleeping; `ListerJob`'s finish timestamps use the same
//! idea here. Production code uses [`MonotonicClock`]; tests use a fake that a test can advance
//! by hand instead of blocking for real.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// A source of monotonic seconds, injected so time-based logic is testable without real delays.
pub trait Clock: Send + Sync {
    /// Seconds since some fixed but arbitrary origin (never wall-clock time), matching
    /// `time.monotonic()`'s contract: only differences between two calls are meaningful.
    fn now(&self) -> f64;
}

/// A cheaply-cloned handle to a clock (real or fake).
pub type SharedClock = Arc<dyn Clock>;

/// The real monotonic clock: seconds elapsed since this instance was created.
#[derive(Debug)]
pub struct MonotonicClock {
    start: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        MonotonicClock { start: Instant::now() }
    }

    /// A [`SharedClock`] wrapping a fresh real clock, for callers that just need the default.
    pub fn shared() -> SharedClock {
        Arc::new(MonotonicClock::new())
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
}

/// A clock a test can set to an exact value instead of waiting for real time to pass.
///
/// Stored as the raw bits of an `f64` in an `AtomicU64` so `set`/`now` need no lock: `Clock` must
/// be `Send + Sync`, and tests set the time from one thread while a background thread reads it.
#[derive(Debug, Default)]
pub struct FakeClock {
    bits: AtomicU64,
}

impl FakeClock {
    pub fn new(t: f64) -> Self {
        FakeClock { bits: AtomicU64::new(t.to_bits()) }
    }

    pub fn set(&self, t: f64) {
        self.bits.store(t.to_bits(), Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> f64 {
        f64::from_bits(self.bits.load(Ordering::SeqCst))
    }
}
