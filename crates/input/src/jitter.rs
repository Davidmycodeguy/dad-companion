//! A tiny, dependency-free source of jitter for macro pacing.
//!
//! `move_mouse_smooth`'s per-step delay jitter (`random.uniform(...)` in the Python reference)
//! only needs to avoid perfectly periodic timing — it is not security-sensitive — so this is a
//! plain SplitMix64 step seeded from the system clock and a counter, not a `rand`-crate RNG. That
//! keeps the crate's dependency list exactly what the task allows (serde, serde_json, thiserror,
//! windows).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A pseudo-random value uniformly distributed over `[0.0, 1.0)`.
pub fn unit_f64() -> f64 {
    let counter = COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let clock = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut z = counter ^ clock;
    // SplitMix64's finalizer: cheap and well-mixed enough for pacing jitter.
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // Top 53 bits give a value with the same precision as an f64 mantissa.
    (z >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_within_unit_range() {
        for _ in 0..1000 {
            let value = unit_f64();
            assert!((0.0..1.0).contains(&value), "{value} out of range");
        }
    }

    #[test]
    fn successive_calls_are_not_all_identical() {
        let samples: std::collections::HashSet<u64> = (0..32).map(|_| unit_f64().to_bits()).collect();
        assert!(samples.len() > 1, "jitter source looks constant");
    }
}
