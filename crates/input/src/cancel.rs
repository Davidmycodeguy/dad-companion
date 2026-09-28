//! Cooperative cancellation for in-flight macros.
//!
//! `macros.py` used a single module-global `threading.Event`, pushed and popped around each macro
//! run (`push_cancel_event`/`pop_cancel_event`/`is_cancelled`), with `_sleep_with_cancel` polling it
//! every 10ms. A global is awkward and unnecessary in Rust: callers just hold a [`Cancel`] (cheaply
//! `Clone`-able, so the UI thread keeps one end and the macro thread the other) and pass it through
//! explicitly. Semantics are otherwise identical, including the 10ms poll granularity.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Error;

/// How often a cancellable sleep wakes up to re-check the flag. Matches `_sleep_with_cancel`'s
/// `time.sleep(min(remaining, 0.01))`.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// A cooperative cancel flag shared between the thread requesting cancellation and the macro
/// thread that must observe it. Cloning shares the same underlying flag.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A fresh, not-yet-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Idempotent, and safe to call from any thread.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// True once [`Cancel::cancel`] has been called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Ports `_ensure_not_cancelled`: `Err(Error::Cancelled)` once cancelled, `Ok(())` otherwise.
    pub fn check(&self) -> Result<(), Error> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Ports `_sleep_with_cancel`: sleeps `duration`, but wakes every [`POLL_INTERVAL`] to check
    /// for cancellation, returning [`Error::Cancelled`] as soon as it is observed instead of
    /// sleeping the full duration.
    pub fn sleep(&self, duration: Duration) -> Result<(), Error> {
        if duration.is_zero() {
            return self.check();
        }

        let deadline = Instant::now() + duration;
        loop {
            self.check()?;
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Ok(());
            };
            if remaining.is_zero() {
                return Ok(());
            }
            std::thread::sleep(remaining.min(POLL_INTERVAL));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_token_is_not_cancelled() {
        let cancel = Cancel::new();
        assert!(!cancel.is_cancelled());
        assert!(cancel.check().is_ok());
    }

    #[test]
    fn cancel_is_observed_by_clones() {
        let cancel = Cancel::new();
        let clone = cancel.clone();
        clone.cancel();
        assert!(cancel.is_cancelled());
        assert!(matches!(cancel.check(), Err(Error::Cancelled)));
    }

    #[test]
    fn sleep_returns_cancelled_once_flagged_instead_of_sleeping_full_duration() {
        let cancel = Cancel::new();
        let flag = cancel.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            flag.cancel();
        });
        let start = Instant::now();
        let result = cancel.sleep(Duration::from_secs(5));
        handle.join().unwrap();
        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn zero_duration_sleep_just_checks_cancellation() {
        let cancel = Cancel::new();
        assert!(cancel.sleep(Duration::ZERO).is_ok());
    }
}
