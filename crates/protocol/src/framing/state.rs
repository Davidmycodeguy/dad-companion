//! Per-stream reassembly state, and the types [`FramedPacketStreams`
//! ](super::FramedPacketStreams) produces: completed frames and desync
//! events.

use std::collections::BTreeMap;
use std::fmt;

/// One TCP stream's reassembly state. The type is `pub(super)` (visible
/// throughout [`super`](crate::framing) and its `reassembly` submodule,
/// which implements the algorithm that reads and mutates it) but not
/// outside this crate: everything else reaches it only through
/// [`FramedPacketStreams`](super::FramedPacketStreams)'s methods.
#[derive(Debug)]
pub(super) struct StreamState {
    /// Contiguous bytes received so far, not yet cut into complete frames.
    pub(super) frame_buffer: Vec<u8>,
    /// The unwrapped sequence number of the next byte this stream expects.
    pub(super) next_sequence: Option<i64>,
    /// Reference point for unwrapping raw (32-bit) sequence numbers, tracked
    /// separately from `next_sequence` so a baseline established from an
    /// out-of-order segment does not move once earlier bytes arrive.
    pub(super) sequence_anchor: Option<i64>,
    /// Segments received ahead of `next_sequence`, keyed by their unwrapped
    /// start sequence, waiting for the gap before them to be filled.
    pub(super) pending_segments: BTreeMap<i64, Vec<u8>>,
    /// Sum of `pending_segments` values' lengths, tracked incrementally so
    /// enforcing `max_pending_bytes` never has to re-sum the map.
    pub(super) pending_bytes: usize,
    /// When the current gap started, for `FramingConfig::gap_timeout`.
    pub(super) gap_started: Option<f64>,
    /// Last time this stream was fed data, for `FramingConfig::idle_timeout`.
    pub(super) last_seen: f64,
}

impl StreamState {
    pub(super) fn new(now: f64) -> Self {
        Self {
            frame_buffer: Vec::new(),
            next_sequence: None,
            sequence_anchor: None,
            pending_segments: BTreeMap::new(),
            pending_bytes: 0,
            gap_started: None,
            last_seen: now,
        }
    }
}

/// One complete, still-undecoded frame: header (8 bytes) plus body.
/// Produced by the low-level, plain-bytes
/// [`FramedPacketStreams::feed`](super::FramedPacketStreams::feed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub packet_type: u16,
    /// The full frame, including its 8-byte header.
    pub bytes: Vec<u8>,
}

/// Why some bytes of a stream were dropped without becoming a [`RawFrame`].
/// The [`fmt::Display`] text matches the Python reference's desync reason
/// strings exactly, for anyone logging these events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesyncReason {
    TooManyActiveStreams,
    InvalidFrameHeader,
    ResynchronizedFrameHeader,
    PendingGapTimedOut,
    PendingGapExceededMemoryLimit,
    GlobalMemoryLimitExceeded,
}

impl DesyncReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TooManyActiveStreams => "too many active TCP streams",
            Self::InvalidFrameHeader => "invalid frame header",
            Self::ResynchronizedFrameHeader => "resynchronized frame header",
            Self::PendingGapTimedOut => "pending TCP gap timed out",
            Self::PendingGapExceededMemoryLimit => "pending TCP gap exceeded memory limit",
            Self::GlobalMemoryLimitExceeded => "global TCP reassembly memory limit exceeded",
        }
    }
}

impl fmt::Display for DesyncReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stream desynchronization: `dropped` bytes of `stream` were discarded
/// because of `reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desync<K> {
    pub stream: K,
    pub dropped: usize,
    pub reason: DesyncReason,
}

pub(super) fn push_desync<K>(
    outcome: &mut FeedOutcome<K>,
    stream: K,
    dropped: usize,
    reason: DesyncReason,
) {
    outcome.desyncs.push(Desync {
        stream,
        dropped,
        reason,
    });
}

/// Everything one
/// [`FramedPacketStreams::feed`](super::FramedPacketStreams::feed) call
/// produced: the frames it completed (in emission order) and any desync
/// events along the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedOutcome<K> {
    pub frames: Vec<RawFrame>,
    pub desyncs: Vec<Desync<K>>,
}

// Written by hand instead of `#[derive(Default)]`, which would add an
// unnecessary `K: Default` bound to the generated impl.
impl<K> Default for FeedOutcome<K> {
    fn default() -> Self {
        Self {
            frames: Vec::new(),
            desyncs: Vec::new(),
        }
    }
}
