//! A faithful port of the Python reference's `FramedPacketStreams`
//! (`UI/src/models/packet_buffers.py`): per-stream TCP reassembly with
//! sequence-number unwrapping, out-of-order pending data, gap recovery after
//! a timeout, header-resynchronization on desync, and memory limits.
//!
//! Segments from different connections never share framing state, and
//! retransmitted or overlapping bytes are never handed to the decoder
//! twice -- that property is what this module (and its ported test suite)
//! exists to protect.
//!
//! Two differences from the Python, both a consequence of Rust's static
//! typing rather than a behavior change:
//! - Python reads the wall clock itself (`time.monotonic()`) and tests
//!   monkeypatch that function to control it. Rust has no equivalent
//!   monkeypatch story, so every entry point here takes `now: f64` (seconds,
//!   any fixed origin) explicitly; [`FramedPacketStreams::feed_segment`]
//!   passes the captured [`Segment`]'s own `time` field.
//! - `feed`'s `sequence` is `Option<u32>`, which cannot hold a value outside
//!   `0..=u32::MAX`, so the `TypeError`/`ValueError` fallback in Python's
//!   `feed` (for a non-integer-like `sequence`) has no Rust equivalent to
//!   port: it is unreachable by construction.
//!
//! Split into submodules to keep each file focused: [`config`] (limits and
//! their validation), `state` (crate-private: per-stream state and the
//! output types), and `reassembly` (crate-private: the sequence-driven
//! algorithm itself). This file holds the public [`FramedPacketStreams`]
//! type and the [`Segment`]-specific adapter on top of it.

mod config;
mod reassembly;
mod state;

pub use config::{
    ConfigError, FramingConfig, DEFAULT_GAP_TIMEOUT, DEFAULT_IDLE_TIMEOUT,
    DEFAULT_MAX_FRAMES_PER_FEED, DEFAULT_MAX_PACKET_SIZE, DEFAULT_MAX_PENDING_BYTES,
    DEFAULT_MAX_STREAMS, DEFAULT_MAX_TOTAL_BUFFERED_BYTES, MIN_GAP_TIMEOUT,
};
pub use state::{Desync, DesyncReason, FeedOutcome, RawFrame};

use std::hash::Hash;

use indexmap::IndexMap;

use reassembly::{buffered_bytes, enforce_total_buffer_limit, expire_idle_streams, process_sequenced_feed, touch_stream};
use state::StreamState;

use crate::segment::{Direction, Segment, StreamKey};

/// Size of a packet header: `<IHH` = u32 length + u16 type + u16 padding.
const HEADER_LEN: usize = 8;

/// TCP sequence numbers are 32-bit; unwrapping maps them onto a 64-bit line.
const SEQUENCE_MODULUS: i64 = 1 << 32;

/// `feed` re-checks idle streams every this-many calls, not every call.
const FEED_COUNT_EXPIRY_INTERVAL: u64 = 128;

/// Reassembles sequence-aware byte streams (keyed by `K`) into
/// length-prefixed frames. See the module documentation for the behavior
/// this ports from the Python reference.
///
/// `K` identifies a stream (a `StreamKey` for [`feed_segment`
/// ](FramedPacketStreams::feed_segment), a plain `String` in the ported
/// unit tests). `V` validates a candidate header
/// `(length, packet_type, padding)`.
#[derive(Debug)]
pub struct FramedPacketStreams<K, V> {
    validate_header: V,
    config: FramingConfig,
    streams: IndexMap<K, StreamState>,
    feed_count: u64,
}

impl<K, V> FramedPacketStreams<K, V>
where
    K: Eq + Hash + Clone,
    V: Fn(u32, u16, u16) -> bool,
{
    /// Creates a reassembler, rejecting the same non-positive limits and
    /// too-short `gap_timeout` the Python constructor does.
    pub fn new(validate_header: V, config: FramingConfig) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            validate_header,
            config,
            streams: IndexMap::new(),
            feed_count: 0,
        })
    }

    /// [`Self::new`] with [`FramingConfig::default`], which always validates
    /// successfully (see `config`'s `default_config_is_valid` test).
    pub fn with_defaults(validate_header: V) -> Self {
        Self::new(validate_header, FramingConfig::default())
            .expect("FramingConfig::default() always passes validation")
    }

    /// Drops all streams' state.
    pub fn clear_all(&mut self) {
        self.streams.clear();
    }

    /// Drops one stream's state, if any.
    pub fn clear_stream(&mut self, stream_id: &K) {
        self.streams.shift_remove(stream_id);
    }

    /// The bytes buffered so far for `stream_id`, not yet cut into frames.
    /// Empty if the stream is unknown, same as the Python reference.
    #[must_use]
    pub fn buffer(&self, stream_id: &K) -> &[u8] {
        self.streams
            .get(stream_id)
            .map_or(&[], |state| state.frame_buffer.as_slice())
    }

    /// Total bytes held across every stream: buffered frame data plus
    /// pending (out-of-order) segments.
    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        buffered_bytes(&self.streams)
    }

    /// How many streams currently have state.
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }

    /// Feeds `data` for `stream_id`, returning the frames it completed and
    /// any desync events. `sequence` is the TCP sequence number of `data`'s
    /// first byte, when known; `out_of_order` mirrors tshark's analysis
    /// flag (see the module documentation for why [`Self::feed_segment`]
    /// cannot supply a real one). `now` is a monotonic clock reading in
    /// seconds, any fixed origin, used for idle and gap timeouts.
    pub fn feed(
        &mut self,
        stream_id: K,
        data: &[u8],
        sequence: Option<u32>,
        out_of_order: bool,
        now: f64,
    ) -> FeedOutcome<K> {
        let mut outcome = FeedOutcome::default();
        if data.is_empty() {
            return outcome;
        }

        touch_stream(
            &mut self.streams,
            &stream_id,
            now,
            self.config.max_streams,
            &mut outcome,
        );

        self.feed_count += 1;
        if self.feed_count.is_multiple_of(FEED_COUNT_EXPIRY_INTERVAL) {
            expire_idle_streams(&mut self.streams, now, self.config.idle_timeout);
        }

        let config = self.config;
        let validate_header = &self.validate_header;
        let state = self
            .streams
            .get_mut(&stream_id)
            .expect("touch_stream just inserted this key");

        process_sequenced_feed(
            config,
            validate_header,
            stream_id,
            state,
            data,
            sequence,
            out_of_order,
            now,
            &mut outcome,
        );

        enforce_total_buffer_limit(&mut self.streams, config.max_total_buffered_bytes, &mut outcome);
        outcome
    }
}

/// One complete packet from a captured [`Segment`] stream: header stripped,
/// tagged with which connection and direction it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub stream: StreamKey,
    pub direction: Direction,
    pub packet_type: u16,
    /// The packet's body: frame bytes after the 8-byte header.
    pub body: Vec<u8>,
}

/// What one [`FramedPacketStreams::feed_segment`] call produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentFeedOutcome {
    pub packets: Vec<Packet>,
    pub desyncs: Vec<Desync<StreamKey>>,
}

impl<V> FramedPacketStreams<StreamKey, V>
where
    V: Fn(u32, u16, u16) -> bool,
{
    /// Feeds one captured [`Segment`], returning the packets it completed
    /// (header stripped) and any desync events. Mirrors `process_packet` in
    /// the Python reference, which likewise just forwards to `feed` and
    /// leaves recovery from unreliable TCP metadata to the reassembly
    /// logic itself.
    ///
    /// Always passes `out_of_order = false`: the `capture` crate does not
    /// surface tshark's out-of-order analysis flag, and there is no way to
    /// derive it from a single segment's own sequence number alone (see the
    /// module documentation). The first segment observed for a stream is
    /// therefore always trusted as that stream's baseline.
    pub fn feed_segment(&mut self, segment: &Segment) -> SegmentFeedOutcome {
        const NOT_OUT_OF_ORDER: bool = false;
        let raw = self.feed(
            segment.stream,
            &segment.payload,
            Some(segment.seq),
            NOT_OUT_OF_ORDER,
            segment.time,
        );
        SegmentFeedOutcome {
            packets: raw
                .frames
                .into_iter()
                .map(|frame| Packet {
                    stream: segment.stream,
                    direction: segment.direction,
                    packet_type: frame.packet_type,
                    body: frame.bytes[HEADER_LEN..].to_vec(),
                })
                .collect(),
            desyncs: raw.desyncs,
        }
    }
}
