//! The reassembly algorithm itself: turning a keyed stream of arbitrary
//! byte chunks (each optionally tagged with a TCP sequence number) into
//! complete, length-prefixed frames. Ported from the Python reference's
//! `FramedPacketStreams` private methods -- see [`super`] for the public
//! API this backs and the behavior it preserves.

use std::hash::Hash;

use indexmap::IndexMap;

use super::config::FramingConfig;
use super::state::{push_desync, DesyncReason, FeedOutcome, RawFrame, StreamState};
use super::{HEADER_LEN, SEQUENCE_MODULUS};

/// Ensures `stream_id` has state (creating it if new) and marks it
/// most-recently-used, evicting the least-recently-used stream(s) while
/// over `max_streams`. Mirrors `_get_state` in the Python reference; unlike
/// it, this also stamps `last_seen = now` directly (the Python caller does
/// that itself right after `_get_state` returns).
pub(super) fn touch_stream<K: Eq + Hash + Clone>(
    streams: &mut IndexMap<K, StreamState>,
    stream_id: &K,
    now: f64,
    max_streams: usize,
    outcome: &mut FeedOutcome<K>,
) {
    let (owned_key, state) = match streams.shift_remove_entry(stream_id) {
        Some((owned_key, mut state)) => {
            state.last_seen = now;
            (owned_key, state)
        }
        None => (stream_id.clone(), StreamState::new(now)),
    };
    streams.insert(owned_key, state);

    while streams.len() > max_streams {
        let Some((evicted_key, evicted_state)) = streams.shift_remove_index(0) else {
            break;
        };
        let dropped = evicted_state.frame_buffer.len() + evicted_state.pending_bytes;
        if dropped > 0 {
            push_desync(outcome, evicted_key, dropped, DesyncReason::TooManyActiveStreams);
        }
    }
}

/// Drops streams that have not been fed data in over `idle_timeout`
/// seconds. Mirrors `_expire_idle_streams`; not notified as a desync there
/// either (idle expiry is routine, not a sign of lost data in flight).
pub(super) fn expire_idle_streams<K: Eq + Hash>(
    streams: &mut IndexMap<K, StreamState>,
    now: f64,
    idle_timeout: f64,
) {
    streams.retain(|_key, state| now - state.last_seen <= idle_timeout);
}

pub(super) fn buffered_bytes<K>(streams: &IndexMap<K, StreamState>) -> usize {
    streams
        .values()
        .map(|state| state.frame_buffer.len() + state.pending_bytes)
        .sum()
}

/// Evicts least-recently-used streams while the total buffered byte count
/// (across every stream) exceeds `max_total_buffered_bytes`. Mirrors
/// `_enforce_total_buffer_limit`, called once per `feed` via `_finish_feed`
/// in the Python reference; here it is simply the last step of
/// [`FramedPacketStreams::feed`](super::FramedPacketStreams::feed).
pub(super) fn enforce_total_buffer_limit<K: Eq + Hash>(
    streams: &mut IndexMap<K, StreamState>,
    max_total_buffered_bytes: usize,
    outcome: &mut FeedOutcome<K>,
) {
    let mut total = buffered_bytes(streams);
    while !streams.is_empty() && total > max_total_buffered_bytes {
        let Some((evicted_key, evicted_state)) = streams.shift_remove_index(0) else {
            break;
        };
        let evicted_bytes = evicted_state.frame_buffer.len() + evicted_state.pending_bytes;
        total = total.saturating_sub(evicted_bytes);
        push_desync(
            outcome,
            evicted_key,
            evicted_bytes,
            DesyncReason::GlobalMemoryLimitExceeded,
        );
    }
}

/// The sequence-number-driven heart of `feed`, once `state` is known to
/// exist: establishes this stream's baseline sequence from the first
/// segment seen, resolves overlap with already-seen bytes, and stores or
/// drains out-of-order data. Returns the number of frames emitted (frames
/// themselves land in `outcome.frames`). Mirrors `feed`'s body in the
/// Python reference from `segment_start = self._unwrap_sequence(...)`
/// onward (the part before it -- state lookup and the plain-bytes shortcut
/// for `sequence is None` -- is handled by the caller and by falling
/// through to [`feed_contiguous`] below when `sequence` is `None`).
#[allow(clippy::too_many_arguments)]
pub(super) fn process_sequenced_feed<K: Eq + Hash + Clone>(
    config: FramingConfig,
    validate_header: &impl Fn(u32, u16, u16) -> bool,
    key: K,
    state: &mut StreamState,
    payload: &[u8],
    sequence: Option<u32>,
    out_of_order: bool,
    now: f64,
    outcome: &mut FeedOutcome<K>,
) -> usize {
    let Some(sequence) = sequence else {
        return feed_contiguous(config, validate_header, key, state, payload, outcome);
    };
    let mut segment_start = unwrap_sequence(state, i64::from(sequence));

    // tshark can deliver a later segment first and mark it out-of-order. Do
    // not make that segment the stream's baseline: doing so would classify
    // the still-missing earlier bytes as a stale retransmission once they
    // arrive. Hold it under the same bounded gap budget used once a
    // baseline exists, unless it fills a gap ahead of an already-pending
    // (also out-of-order) earlier segment.
    if state.next_sequence.is_none() && out_of_order {
        let fills_earlier_gap = state
            .pending_segments
            .keys()
            .next()
            .is_some_and(|&smallest_pending| segment_start < smallest_pending);
        if fills_earlier_gap {
            let baseline = segment_start + payload.len() as i64;
            state.next_sequence = Some(baseline);
            state.sequence_anchor = Some(baseline);
            let emitted = feed_contiguous(config, validate_header, key.clone(), state, payload, outcome);
            return emitted + drain_pending(config, validate_header, key, state, now, outcome);
        }
        return store_pending(
            config,
            validate_header,
            key,
            state,
            segment_start,
            payload,
            now,
            outcome,
        );
    }

    let next_sequence = match state.next_sequence {
        None => {
            let baseline = segment_start + payload.len() as i64;
            state.next_sequence = Some(baseline);
            state.sequence_anchor = Some(baseline);
            let emitted = feed_contiguous(config, validate_header, key.clone(), state, payload, outcome);
            return emitted + drain_pending(config, validate_header, key, state, now, outcome);
        }
        Some(next_sequence) => next_sequence,
    };

    let mut payload = payload;
    if segment_start < next_sequence {
        let overlap = next_sequence - segment_start;
        if overlap >= payload.len() as i64 {
            // Fully-seen retransmission: nothing new, just check the gap.
            return recover_expired_gap(config, validate_header, key, state, now, outcome).unwrap_or(0);
        }
        payload = &payload[overlap as usize..];
        segment_start = next_sequence;
    }

    if segment_start == next_sequence {
        let new_next_sequence = next_sequence + payload.len() as i64;
        state.next_sequence = Some(new_next_sequence);
        state.sequence_anchor = Some(new_next_sequence);
        let mut emitted = feed_contiguous(config, validate_header, key.clone(), state, payload, outcome);
        emitted += drain_pending(config, validate_header, key.clone(), state, now, outcome);
        if emitted > 0 {
            return emitted;
        }
        return emitted + recover_expired_gap(config, validate_header, key, state, now, outcome).unwrap_or(0);
    }

    // A gap remains before `segment_start`: hold the segment until the
    // missing sequence range arrives.
    store_pending(
        config,
        validate_header,
        key,
        state,
        segment_start,
        payload,
        now,
        outcome,
    )
}

/// Appends `data` to `state`'s frame buffer and cuts off as many complete,
/// valid frames as it can (up to `max_frames_per_feed`), resynchronizing on
/// an invalid header by searching for the next plausible one. Mirrors
/// `_feed_contiguous`. Called with an empty `data` slice to retry cutting
/// frames from already-buffered bytes without appending anything new.
fn feed_contiguous<K: Clone>(
    config: FramingConfig,
    validate_header: &impl Fn(u32, u16, u16) -> bool,
    key: K,
    state: &mut StreamState,
    data: &[u8],
    outcome: &mut FeedOutcome<K>,
) -> usize {
    state.frame_buffer.extend_from_slice(data);
    let mut emitted = 0usize;

    while emitted < config.max_frames_per_feed {
        if state.frame_buffer.len() < HEADER_LEN {
            break;
        }
        let (packet_length, packet_type, padding) = read_header(&state.frame_buffer);
        let header_valid = packet_length as usize <= config.max_packet_size
            && validate_header(packet_length, packet_type, padding);

        if !header_valid {
            match find_next_header(&state.frame_buffer, config.max_packet_size, validate_header) {
                None => {
                    // No plausible header anywhere in the buffer. Keep only
                    // the trailing bytes that could still be the start of
                    // one once more data arrives.
                    let dropped = state.frame_buffer.len().saturating_sub(HEADER_LEN - 1);
                    if dropped > 0 {
                        state.frame_buffer.drain(..dropped);
                        push_desync(outcome, key.clone(), dropped, DesyncReason::InvalidFrameHeader);
                    }
                    break;
                }
                Some(offset) => {
                    state.frame_buffer.drain(..offset);
                    push_desync(
                        outcome,
                        key.clone(),
                        offset,
                        DesyncReason::ResynchronizedFrameHeader,
                    );
                    continue;
                }
            }
        }

        let packet_length = packet_length as usize;
        if state.frame_buffer.len() < packet_length {
            break;
        }

        let bytes: Vec<u8> = state.frame_buffer.drain(..packet_length).collect();
        outcome.frames.push(RawFrame { packet_type, bytes });
        emitted += 1;
    }

    emitted
}

/// Reads the 8-byte `<IHH` header at the start of `buffer`.
///
/// Precondition: `buffer.len() >= HEADER_LEN`, always checked by the caller
/// immediately beforehand -- the `try_into` calls below cannot fail.
fn read_header(buffer: &[u8]) -> (u32, u16, u16) {
    let length = u32::from_le_bytes(
        buffer[0..4]
            .try_into()
            .expect("caller checked buffer.len() >= HEADER_LEN"),
    );
    let packet_type = u16::from_le_bytes(
        buffer[4..6]
            .try_into()
            .expect("caller checked buffer.len() >= HEADER_LEN"),
    );
    let padding = u16::from_le_bytes(
        buffer[6..8]
            .try_into()
            .expect("caller checked buffer.len() >= HEADER_LEN"),
    );
    (length, packet_type, padding)
}

/// Searches `data` (from offset 1 onward -- offset 0 was just rejected by
/// the caller) for the first offset holding a plausible header, so framing
/// can resynchronize instead of waiting forever on a corrupt stream.
/// Mirrors `_find_next_header`.
fn find_next_header(
    data: &[u8],
    max_packet_size: usize,
    validate_header: &impl Fn(u32, u16, u16) -> bool,
) -> Option<usize> {
    if data.len() < HEADER_LEN {
        return None;
    }
    let last_start = data.len() - HEADER_LEN;
    (1..=last_start).find(|&offset| {
        let (length, packet_type, padding) = read_header(&data[offset..]);
        length as usize <= max_packet_size && validate_header(length, packet_type, padding)
    })
}

/// Drains pending out-of-order segments that `next_sequence` has now caught
/// up to, one at a time (each may itself advance `next_sequence` and expose
/// the next one), stopping when the smallest remaining pending start is
/// still ahead of `next_sequence`. Mirrors `_drain_pending`, including its
/// gap-timer bookkeeping: a gap that is fully drained is cleared, and a gap
/// that merely shrinks to reveal a *different* missing range gets a fresh
/// timeout rather than inheriting the old one's age.
fn drain_pending<K: Clone>(
    config: FramingConfig,
    validate_header: &impl Fn(u32, u16, u16) -> bool,
    key: K,
    state: &mut StreamState,
    now: f64,
    outcome: &mut FeedOutcome<K>,
) -> usize {
    let initial_smallest_pending = state.pending_segments.keys().next().copied();
    let mut emitted = 0usize;

    while let Some(next_sequence) = state.next_sequence {
        let Some((&start, _)) = state.pending_segments.iter().next() else {
            break;
        };
        if start > next_sequence {
            break;
        }
        let segment = state
            .pending_segments
            .remove(&start)
            .expect("start was just read from this map's own smallest key");
        state.pending_bytes -= segment.len();

        let end = start + segment.len() as i64;
        if end <= next_sequence {
            // Fully covered by data already reassembled; discard and look
            // at the next-smallest pending segment.
            continue;
        }

        let new_bytes = &segment[(next_sequence - start) as usize..];
        let new_next_sequence = next_sequence + new_bytes.len() as i64;
        state.next_sequence = Some(new_next_sequence);
        state.sequence_anchor = Some(new_next_sequence);
        emitted += feed_contiguous(config, validate_header, key.clone(), state, new_bytes, outcome);
    }

    if state.pending_segments.is_empty() {
        state.gap_started = None;
    } else if state.pending_segments.keys().next().copied() != initial_smallest_pending {
        state.gap_started = Some(now);
    }

    emitted
}

/// Holds `payload` (received at unwrapped sequence `segment_start`, ahead of
/// what this stream can use yet) until the gap before it is filled or times
/// out, replacing any shorter segment already held for the same start.
/// Enforces `max_pending_bytes` by discarding everything pending and
/// resuming from the newest segment once the budget is exceeded, since a
/// permanently missing segment must not grow memory without bound. Mirrors
/// `_store_pending`.
#[allow(clippy::too_many_arguments)]
fn store_pending<K: Clone>(
    config: FramingConfig,
    validate_header: &impl Fn(u32, u16, u16) -> bool,
    key: K,
    state: &mut StreamState,
    segment_start: i64,
    payload: &[u8],
    now: f64,
    outcome: &mut FeedOutcome<K>,
) -> usize {
    if state.gap_started.is_none() {
        state.gap_started = Some(now);
    }

    let previous_len = state.pending_segments.get(&segment_start).map(Vec::len);
    let is_new_or_longer = previous_len.is_none_or(|previous_len| payload.len() > previous_len);
    if is_new_or_longer {
        if let Some(previous_len) = previous_len {
            state.pending_bytes -= previous_len;
        }
        state.pending_segments.insert(segment_start, payload.to_vec());
        state.pending_bytes += payload.len();
        if state.sequence_anchor.is_none() {
            state.sequence_anchor = Some(segment_start);
        }
    }

    if state.pending_bytes <= config.max_pending_bytes {
        return recover_expired_gap(config, validate_header, key, state, now, outcome).unwrap_or(0);
    }

    push_desync(
        outcome,
        key.clone(),
        state.pending_bytes,
        DesyncReason::PendingGapExceededMemoryLimit,
    );
    let (newest_start, newest) = state
        .pending_segments
        .pop_last()
        .expect("pending_bytes > max_pending_bytes >= 1 implies at least one entry");
    state.pending_segments.clear();
    state.pending_bytes = 0;
    state.gap_started = None;
    state.frame_buffer.clear();
    let new_next_sequence = newest_start + newest.len() as i64;
    state.next_sequence = Some(new_next_sequence);
    state.sequence_anchor = Some(new_next_sequence);
    feed_contiguous(config, validate_header, key, state, &newest, outcome)
}

/// If the current gap has been pending for at least `gap_timeout` seconds,
/// gives up on it: preserves any complete frames the frame budget left
/// buffered if there are any, otherwise resynchronizes from the earliest
/// pending segment and drains whatever that newly unblocks. Returns `None`
/// when there is no gap to recover yet (nothing pending, or not timed out),
/// matching Python's use of `None` to mean "did not run" as opposed to
/// `Some(0)` meaning "ran, emitted nothing". Mirrors `_recover_expired_gap`.
fn recover_expired_gap<K: Clone>(
    config: FramingConfig,
    validate_header: &impl Fn(u32, u16, u16) -> bool,
    key: K,
    state: &mut StreamState,
    now: f64,
    outcome: &mut FeedOutcome<K>,
) -> Option<usize> {
    let gap_started = state.gap_started?;
    if state.pending_segments.is_empty() || now - gap_started < config.gap_timeout {
        return None;
    }

    let emitted = feed_contiguous(config, validate_header, key.clone(), state, &[], outcome);
    if emitted > 0 {
        return Some(emitted);
    }

    push_desync(
        outcome,
        key.clone(),
        state.frame_buffer.len(),
        DesyncReason::PendingGapTimedOut,
    );
    state.frame_buffer.clear();
    let smallest_pending = *state
        .pending_segments
        .keys()
        .next()
        .expect("checked pending_segments is non-empty above");
    state.next_sequence = Some(smallest_pending);
    state.sequence_anchor = Some(smallest_pending);
    state.gap_started = Some(now);
    Some(emitted + drain_pending(config, validate_header, key, state, now, outcome))
}

/// Maps a raw (32-bit-wrapping) TCP sequence number onto the nearest point
/// on the stream's unwrapped 64-bit line, so a wraparound never looks like
/// a multi-gigabyte gap. The first sequence number ever seen for a stream
/// becomes its anchor with no unwrapping needed. Mirrors `_unwrap_sequence`.
///
/// `sequence` is always in `0..SEQUENCE_MODULUS` given today's callers (it
/// comes from a real `u32`), so the out-of-range early return can't
/// currently trigger; it is kept for parity with the Python reference,
/// which guards the same way for a raw, not-yet-validated input.
fn unwrap_sequence(state: &mut StreamState, sequence: i64) -> i64 {
    if !(0..SEQUENCE_MODULUS).contains(&sequence) {
        return sequence;
    }

    let reference = match state.next_sequence.or(state.sequence_anchor) {
        Some(reference) => reference,
        None => {
            state.sequence_anchor = Some(sequence);
            return sequence;
        }
    };

    let cycle = reference.div_euclid(SEQUENCE_MODULUS) * SEQUENCE_MODULUS;
    let candidates = [
        sequence + cycle - SEQUENCE_MODULUS,
        sequence + cycle,
        sequence + cycle + SEQUENCE_MODULUS,
    ];
    candidates
        .into_iter()
        .min_by_key(|&candidate| (candidate - reference).unsigned_abs())
        .expect("candidates is a fixed non-empty array")
}
