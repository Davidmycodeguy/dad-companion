//! Ported 1:1 from the `FramedPacketStreams` tests in the Python
//! reference's `UI/tests/test_packet_buffers.py`. That file also tests
//! `BoundedPacketHistory`, `estimate_json_size`, `PacketCapture` and
//! `MemoryGuard`; none of those are implemented by this crate (out of
//! scope: history/UI buffering, live capture and memory-guard policy live
//! elsewhere), so only the framing tests are ported here.
//!
//! Test names drop the redundant `test_` prefix the Python file uses
//! (`#[test]` already marks these), but are otherwise unchanged so each one
//! can be matched back to its Python original by name.

use protocol::framing::{ConfigError, Desync, DesyncReason, FeedOutcome, FramedPacketStreams, FramingConfig};

/// `_frame` from the Python tests: an 8-byte `<IHH` header (length includes
/// the header itself) followed by `payload`.
fn frame(packet_type: u16, payload: &[u8]) -> Vec<u8> {
    frame_with_padding(packet_type, payload, 0)
}

fn frame_with_padding(packet_type: u16, payload: &[u8], padding: u16) -> Vec<u8> {
    let length = u32::try_from(payload.len() + 8).expect("test payload fits in a u32 length");
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&packet_type.to_le_bytes());
    bytes.extend_from_slice(&padding.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

/// `_validator` from the Python tests.
fn validator(length: u32, packet_type: u16, padding: u16) -> bool {
    (8..=4096).contains(&length)
        && matches!(packet_type, 1352 | 1354 | 1401)
        && matches!(padding, 0 | 256)
}

/// Shorthand for `FramedPacketStreams::new` with the shared `validator`,
/// only overriding `max_packet_size` (every non-timed test only ever
/// overrides that one limit).
fn streams_with_max_packet_size(
    max_packet_size: usize,
) -> FramedPacketStreams<String, impl Fn(u32, u16, u16) -> bool> {
    let config = FramingConfig {
        max_packet_size,
        ..FramingConfig::default()
    };
    FramedPacketStreams::new(validator, config).expect("test config is valid")
}

/// Extends `captured` with `(packet_type, body)` per completed frame,
/// mirroring `lambda packet, proto: captured.append((proto, packet[8:]))`.
fn extend_with_bodies(captured: &mut Vec<(u16, Vec<u8>)>, outcome: FeedOutcome<String>) {
    for raw_frame in outcome.frames {
        captured.push((raw_frame.packet_type, raw_frame.bytes[8..].to_vec()));
    }
}

#[test]
fn interleaved_tcp_streams_keep_independent_frame_buffers() {
    let mut streams = streams_with_max_packet_size(4096);
    let merchant_list = frame(1352, b"merchant-list");
    let stock_list = frame(1354, b"stock-list");
    let mut captured = Vec::new();

    extend_with_bodies(
        &mut captured,
        streams.feed("7".to_string(), &merchant_list[..11], Some(100), false, 0.0),
    );
    extend_with_bodies(
        &mut captured,
        streams.feed("8".to_string(), &stock_list, Some(500), false, 0.0),
    );
    extend_with_bodies(
        &mut captured,
        streams.feed("7".to_string(), &merchant_list[11..], Some(111), false, 0.0),
    );

    assert_eq!(
        captured,
        vec![
            (1354, b"stock-list".to_vec()),
            (1352, b"merchant-list".to_vec()),
        ]
    );
    assert_eq!(streams.buffered_bytes(), 0);
}

#[test]
fn retransmitted_and_overlapping_segments_are_not_decoded_twice() {
    let mut streams = streams_with_max_packet_size(4096);
    let header_sized_response = frame(1354, b"done");
    let mut captured: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut collect_full = |outcome: FeedOutcome<String>| {
        for raw_frame in outcome.frames {
            captured.push((raw_frame.packet_type, raw_frame.bytes));
        }
    };

    collect_full(streams.feed(
        "3".to_string(),
        &header_sized_response[..10],
        Some(1000),
        false,
        0.0,
    ));
    // Starts two bytes before next_sequence. Only the unseen suffix is used.
    collect_full(streams.feed(
        "3".to_string(),
        &header_sized_response[8..],
        Some(1008),
        false,
        0.0,
    ));
    // Full TCP retransmission must not create another 12-byte response.
    collect_full(streams.feed(
        "3".to_string(),
        &header_sized_response,
        Some(1000),
        false,
        0.0,
    ));

    assert_eq!(captured, vec![(1354, header_sized_response)]);
    assert_eq!(streams.buffered_bytes(), 0);
}

#[test]
fn out_of_order_segment_waits_for_gap_then_drains_in_sequence() {
    let mut streams = streams_with_max_packet_size(4096);
    let quest_list = frame(1401, b"quest-payload");
    let mut captured = Vec::new();

    extend_with_bodies(
        &mut captured,
        streams.feed("quests".to_string(), &quest_list[..6], Some(50), false, 0.0),
    );
    extend_with_bodies(
        &mut captured,
        streams.feed("quests".to_string(), &quest_list[10..], Some(60), false, 0.0),
    );
    assert!(captured.is_empty());

    extend_with_bodies(
        &mut captured,
        streams.feed("quests".to_string(), &quest_list[6..10], Some(56), false, 0.0),
    );

    assert_eq!(captured, vec![(1401, b"quest-payload".to_vec())]);
    assert_eq!(streams.buffered_bytes(), 0);
}

#[test]
fn first_observed_out_of_order_segment_does_not_become_baseline() {
    let mut streams = streams_with_max_packet_size(4096);
    let quest_list = frame(1401, b"quest-payload");
    let mut captured = Vec::new();

    extend_with_bodies(
        &mut captured,
        streams.feed("quests".to_string(), &quest_list[10..], Some(60), true, 0.0),
    );
    assert!(captured.is_empty());

    // Gap-filling packets may also be tagged out-of-order by tshark.
    extend_with_bodies(
        &mut captured,
        streams.feed("quests".to_string(), &quest_list[..10], Some(50), true, 0.0),
    );

    assert_eq!(captured, vec![(1401, b"quest-payload".to_vec())]);
    assert_eq!(streams.buffered_bytes(), 0);
}

#[test]
fn tcp_sequence_wraparound_keeps_segments_contiguous() {
    let mut streams = streams_with_max_packet_size(4096);
    let merchant_list = frame(1352, b"wrapped");
    let first_start = u32::MAX - 3; // (1 << 32) - 4
    let mut captured = Vec::new();

    extend_with_bodies(
        &mut captured,
        streams.feed(
            "merchant".to_string(),
            &merchant_list[..6],
            Some(first_start),
            false,
            0.0,
        ),
    );
    extend_with_bodies(
        &mut captured,
        streams.feed("merchant".to_string(), &merchant_list[6..], Some(2), false, 0.0),
    );

    assert_eq!(captured, vec![(1352, b"wrapped".to_vec())]);
    assert_eq!(streams.buffered_bytes(), 0);
}

#[test]
fn decoder_resynchronizes_when_capture_starts_mid_packet() {
    let mut streams = streams_with_max_packet_size(4096);
    let mut captured = Vec::new();

    let mut mid_packet_bytes = b"mid-packet-bytes".to_vec();
    mid_packet_bytes.extend_from_slice(&frame(1352, b"ok"));
    let outcome = streams.feed("late".to_string(), &mid_packet_bytes, None, false, 0.0);

    // `_find_next_header` locates the real header right after the garbage
    // prefix, so this resynchronizes rather than giving up on the buffer
    // (that is `InvalidFrameHeader`, for when no header is found at all).
    assert_eq!(outcome.desyncs.len(), 1);
    assert_eq!(outcome.desyncs[0].dropped, b"mid-packet-bytes".len());
    assert_eq!(outcome.desyncs[0].reason, DesyncReason::ResynchronizedFrameHeader);
    extend_with_bodies(&mut captured, outcome);
    assert_eq!(captured, vec![(1352, b"ok".to_vec())]);
}

#[test]
fn stream_reassembly_has_a_global_byte_cap() {
    let config = FramingConfig {
        max_packet_size: 4096,
        max_pending_bytes: 4096,
        max_total_buffered_bytes: 10,
        ..FramingConfig::default()
    };
    let mut streams = FramedPacketStreams::new(validator, config).expect("test config is valid");
    let mut desyncs: Vec<Desync<String>> = Vec::new();

    desyncs.extend(streams.feed("oldest".to_string(), b"12345", None, false, 0.0).desyncs);
    desyncs.extend(streams.feed("middle".to_string(), b"12345", None, false, 0.0).desyncs);
    desyncs.extend(streams.feed("newest".to_string(), b"12345", None, false, 0.0).desyncs);

    assert!(streams.buffered_bytes() <= 10);
    assert_eq!(streams.stream_count(), 2);
    assert_eq!(
        desyncs,
        vec![Desync {
            stream: "oldest".to_string(),
            dropped: 5,
            reason: DesyncReason::GlobalMemoryLimitExceeded,
        }]
    );
}

#[test]
fn gap_timeout_rejects_values_below_minimum_and_nonfinite() {
    let validate_header: fn(u32, u16, u16) -> bool = validator;
    for gap_timeout in [0.0, 10.0, 19.999, f64::NAN, f64::INFINITY] {
        let config = FramingConfig {
            gap_timeout,
            ..FramingConfig::default()
        };
        let error = FramedPacketStreams::<String, _>::new(validate_header, config).unwrap_err();
        assert_eq!(error, ConfigError::GapTimeoutTooSmall);
    }
}

// Gap recovery must remain independent of wall-clock time and real
// sleeping. The Python tests monkeypatch `time.monotonic`; this port passes
// `now` explicitly to `feed` instead, so no monkeypatching is needed --
// `TimedHarness::feed` just takes `now` as a parameter (see the module
// documentation for why `feed` is shaped this way).
struct TimedHarness {
    streams: FramedPacketStreams<String, fn(u32, u16, u16) -> bool>,
    captured: Vec<Vec<u8>>,
    desyncs: Vec<Desync<String>>,
}

impl TimedHarness {
    fn new(config: FramingConfig) -> Self {
        // `validator` (an fn item) needs an explicit cast to the fn
        // *pointer* type named in this struct's field, since the two are
        // distinct types that only coerce automatically in some contexts.
        let validate_header: fn(u32, u16, u16) -> bool = validator;
        Self {
            streams: FramedPacketStreams::new(validate_header, config).expect("test config is valid"),
            captured: Vec::new(),
            desyncs: Vec::new(),
        }
    }

    fn with_defaults() -> Self {
        Self::new(FramingConfig::default())
    }

    /// Mirrors `streams.feed(key, data, sequence=seq)` in the Python tests
    /// (`out_of_order` is never set to `True` by any of them) and returns
    /// the number of frames emitted, like Python's `feed` does.
    fn feed(&mut self, key: &str, data: &[u8], sequence: u32, now: f64) -> usize {
        let outcome = self.streams.feed(key.to_string(), data, Some(sequence), false, now);
        let emitted = outcome.frames.len();
        self.captured.extend(outcome.frames.into_iter().map(|frame| frame.bytes));
        self.desyncs.extend(outcome.desyncs);
        emitted
    }
}

#[test]
fn gap_timeout_never_recovers_before_twenty_seconds() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1401, b"");

    harness.feed("game", &packet, 100, 0.0);
    harness.feed("game", &packet, 116, 0.0); // Missing [108, 116).
    harness.feed("game", &packet, 124, 19.999);
    assert_eq!(harness.captured, vec![packet.clone()]);
    assert_eq!(harness.streams.buffered_bytes(), 16);
    assert!(harness.desyncs.is_empty());

    harness.feed("game", &packet, 132, 20.0);
    assert_eq!(harness.captured, vec![packet.clone(); 4]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
    assert_eq!(
        harness.desyncs,
        vec![Desync {
            stream: "game".to_string(),
            dropped: 0,
            reason: DesyncReason::PendingGapTimedOut,
        }]
    );
}

#[test]
fn gap_recovery_preserves_complete_frames_left_by_frame_budget() {
    let config = FramingConfig {
        max_frames_per_feed: 1,
        ..FramingConfig::default()
    };
    let mut harness = TimedHarness::new(config);
    let packet = frame(1401, b"");
    let incomplete = frame(1352, b"unfinished")[..9].to_vec();
    let mut payload = packet.repeat(3);
    payload.extend_from_slice(&incomplete);

    harness.feed("game", &payload, 100, 0.0);
    let pending_start = 100 + u32::try_from(payload.len()).unwrap() + 8;
    harness.feed("game", &packet, pending_start, 0.0);

    harness.feed("game", &packet, pending_start, 20.0);
    assert_eq!(harness.captured, vec![packet.clone(); 2]);
    assert!(harness.desyncs.is_empty());

    harness.feed("game", &packet, pending_start, 20.0);
    assert_eq!(harness.captured, vec![packet.clone(); 3]);
    assert!(harness.desyncs.is_empty());

    harness.feed("game", &packet, pending_start, 20.0);
    assert_eq!(harness.captured, vec![packet.clone(); 4]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
    assert_eq!(
        harness.desyncs,
        vec![Desync {
            stream: "game".to_string(),
            dropped: incomplete.len(),
            reason: DesyncReason::PendingGapTimedOut,
        }]
    );
}

#[test]
fn timeout_does_not_add_frame_batch_after_contiguous_emission() {
    let config = FramingConfig {
        max_frames_per_feed: 1,
        ..FramingConfig::default()
    };
    let mut harness = TimedHarness::new(config);
    let packet = frame(1401, b"");

    harness.feed("game", &packet, 100, 0.0);
    harness.feed("game", &packet, 124, 0.0);

    assert_eq!(harness.feed("game", &packet, 108, 20.0), 1);
    assert_eq!(harness.captured, vec![packet.clone(); 2]);
    assert!(harness.desyncs.is_empty());

    assert_eq!(harness.feed("game", &packet, 124, 20.0), 1);
    assert_eq!(harness.captured, vec![packet.clone(); 3]);
    assert_eq!(
        harness.desyncs,
        vec![Desync {
            stream: "game".to_string(),
            dropped: 0,
            reason: DesyncReason::PendingGapTimedOut,
        }]
    );
}

#[test]
fn observed_eight_byte_gap_recovers_on_next_feed_after_deadline() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1401, b"");

    harness.feed("game", &packet, 1000, 0.0);
    harness.feed("game", &packet, 1016, 7.0);
    assert_eq!(harness.captured, vec![packet.clone()]); // Time alone does not run recovery.

    harness.feed("game", &packet, 1024, 28.0);
    assert_eq!(harness.captured, vec![packet.clone(); 3]);
    assert_eq!(harness.streams.buffered_bytes(), 0);

    harness.feed("game", &packet, 1008, 28.0); // Late missing bytes cannot replay.
    harness.feed("game", &packet, 1024, 28.0); // Nor can a duplicate.
    let mut overlap_and_new = packet[4..].to_vec();
    overlap_and_new.extend_from_slice(&packet);
    harness.feed("game", &overlap_and_new, 1028, 28.0); // Overlap + new bytes.
    assert_eq!(harness.captured, vec![packet.clone(); 4]);
}

#[test]
fn gap_filled_before_deadline_preserves_partial_frame() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1352, b"complete");

    harness.feed("game", &packet[..8], 100, 0.0);
    harness.feed("game", &packet[10..], 110, 0.0);
    harness.feed("game", &packet[8..10], 108, 19.999);

    assert_eq!(harness.captured, vec![packet]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
    assert!(harness.desyncs.is_empty());
}

#[test]
fn legitimate_slow_partial_frame_has_no_gap_timeout() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1352, b"slow-but-contiguous");

    harness.feed("game", &packet[..8], 100, 0.0);
    harness.feed("game", &packet[8..], 108, 100.0);

    assert_eq!(harness.captured, vec![packet]);
    assert!(harness.desyncs.is_empty());
}

#[test]
fn gap_recovery_discards_incomplete_frame_and_resynchronizes() {
    let mut harness = TimedHarness::with_defaults();
    let incomplete = frame(1352, b"abcdefghijk");
    let fresh = frame(1354, b"confirmed");

    harness.feed("game", &incomplete[..9], 100, 0.0);
    let mut tail = incomplete[12..].to_vec();
    tail.extend_from_slice(&fresh);
    let tail_len = u32::try_from(tail.len()).unwrap();
    harness.feed("game", &tail, 112, 0.0);

    let packet_1401 = frame(1401, b"");
    harness.feed("game", &packet_1401, 112 + tail_len, 20.0);

    assert_eq!(harness.captured, vec![fresh, packet_1401]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
    assert!(harness
        .desyncs
        .iter()
        .any(|event| event.reason == DesyncReason::PendingGapTimedOut));
}

#[test]
fn new_gap_gets_full_timeout_after_first_gap_fills() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1401, b"");

    harness.feed("game", &packet, 100, 0.0);
    harness.feed("game", &packet, 116, 0.0);
    harness.feed("game", &packet, 132, 0.0);
    harness.feed("game", &packet, 108, 19.0); // Drains 116, exposes missing 124.
    harness.feed("game", &packet, 140, 20.0);
    assert_eq!(harness.captured, vec![packet.clone(); 3]);
    assert!(harness.desyncs.is_empty());

    harness.feed("game", &packet, 148, 39.0);
    assert_eq!(harness.captured, vec![packet.clone(); 6]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
}

#[test]
fn gap_timeout_is_per_stream() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1401, b"");

    harness.feed("a", &packet, 100, 0.0);
    harness.feed("a", &packet, 116, 0.0);
    harness.feed("b", &packet, 100, 19.0);
    harness.feed("b", &packet, 116, 19.0);
    harness.feed("a", &packet, 124, 20.0);
    harness.feed("b", &packet, 124, 20.0);

    assert_eq!(harness.captured, vec![packet.clone(); 4]);
    assert_eq!(harness.streams.buffered_bytes(), 16);
    assert!(harness.desyncs.iter().all(|event| event.stream == "a"));
}

#[test]
fn partial_gap_progress_does_not_postpone_original_deadline() {
    let mut harness = TimedHarness::with_defaults();
    let partial = frame(1352, b"12345678");
    let fresh = frame(1401, b"");

    harness.feed("game", &partial[..8], 100, 0.0);
    harness.feed("game", &fresh, 116, 0.0);
    harness.feed("game", &partial[8..12], 108, 19.0);
    assert!(harness.captured.is_empty());
    assert!(harness.desyncs.is_empty());

    harness.feed("game", &partial[12..14], 112, 20.0);
    assert_eq!(harness.captured, vec![fresh]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
    assert_eq!(harness.desyncs[0].reason, DesyncReason::PendingGapTimedOut);
}

#[test]
fn retransmission_feed_checks_gap_deadline() {
    let mut harness = TimedHarness::with_defaults();
    let packet = frame(1401, b"");

    harness.feed("game", &packet, 100, 0.0);
    harness.feed("game", &packet, 116, 0.0);
    harness.feed("game", &packet, 100, 19.999);
    assert_eq!(harness.captured, vec![packet.clone()]);
    assert!(harness.desyncs.is_empty());

    harness.feed("game", &packet, 100, 20.0);
    assert_eq!(harness.captured, vec![packet.clone(); 2]);
    assert_eq!(harness.streams.buffered_bytes(), 0);
}

#[test]
fn evicting_empty_stream_does_not_report_desynchronization() {
    let config = FramingConfig {
        max_streams: 1,
        ..FramingConfig::default()
    };
    let mut harness = TimedHarness::new(config);
    let packet = frame(1401, b"");

    harness.feed("one", &packet, 100, 0.0);
    harness.feed("two", &packet, 100, 0.0);

    assert_eq!(harness.captured, vec![packet.clone(); 2]);
    assert!(harness.desyncs.is_empty());
}
