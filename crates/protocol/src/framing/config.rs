//! Tunable limits for a [`FramedPacketStreams`](super::FramedPacketStreams),
//! matching the Python reference's `FramedPacketStreams.__init__` defaults
//! and validation.

/// Smallest legal `gap_timeout`: recovery is feed-driven, never a
/// free-running timer, but must still wait at least this long.
pub const MIN_GAP_TIMEOUT: f64 = 20.0;

pub const DEFAULT_MAX_PACKET_SIZE: usize = 2 * 1024 * 1024;
pub const DEFAULT_MAX_PENDING_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_MAX_TOTAL_BUFFERED_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_STREAMS: usize = 64;
pub const DEFAULT_IDLE_TIMEOUT: f64 = 300.0;
pub const DEFAULT_MAX_FRAMES_PER_FEED: usize = 4096;
pub const DEFAULT_GAP_TIMEOUT: f64 = MIN_GAP_TIMEOUT;

/// Tunable limits for a [`FramedPacketStreams`](super::FramedPacketStreams).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramingConfig {
    pub max_packet_size: usize,
    pub max_pending_bytes: usize,
    pub max_total_buffered_bytes: usize,
    pub max_streams: usize,
    pub idle_timeout: f64,
    pub max_frames_per_feed: usize,
    pub gap_timeout: f64,
}

impl Default for FramingConfig {
    fn default() -> Self {
        Self {
            max_packet_size: DEFAULT_MAX_PACKET_SIZE,
            max_pending_bytes: DEFAULT_MAX_PENDING_BYTES,
            max_total_buffered_bytes: DEFAULT_MAX_TOTAL_BUFFERED_BYTES,
            max_streams: DEFAULT_MAX_STREAMS,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            max_frames_per_feed: DEFAULT_MAX_FRAMES_PER_FEED,
            gap_timeout: DEFAULT_GAP_TIMEOUT,
        }
    }
}

/// Why a [`FramingConfig`] was rejected by
/// [`FramedPacketStreams::new`](super::FramedPacketStreams::new).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// A byte/count limit was zero. Python also rejected negative limits;
    /// Rust's unsigned integer types already make that impossible here.
    #[error("packet stream limits must be positive")]
    NonPositiveLimit,
    #[error("idle_timeout must be positive")]
    NonPositiveIdleTimeout,
    #[error("gap_timeout must be finite and at least {MIN_GAP_TIMEOUT} seconds")]
    GapTimeoutTooSmall,
}

impl FramingConfig {
    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        let limits_positive = [
            self.max_packet_size,
            self.max_pending_bytes,
            self.max_total_buffered_bytes,
            self.max_streams,
            self.max_frames_per_feed,
        ]
        .into_iter()
        .all(|limit| limit > 0);
        if !limits_positive {
            return Err(ConfigError::NonPositiveLimit);
        }
        if self.idle_timeout <= 0.0 {
            return Err(ConfigError::NonPositiveIdleTimeout);
        }
        if !(self.gap_timeout.is_finite() && self.gap_timeout >= MIN_GAP_TIMEOUT) {
            return Err(ConfigError::GapTimeoutTooSmall);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        FramingConfig::default().validate().unwrap();
    }
}
