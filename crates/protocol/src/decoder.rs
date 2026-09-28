//! Ties [`framing`](crate::framing) and [`messages`](crate::messages)
//! together: feed captured [`Segment`]s in, get decoded messages out.

use crate::framing::{DesyncReason, FramedPacketStreams};
use crate::messages::{self, DecodeError, Message};
use crate::segment::{Segment, StreamKey};

/// The header length/padding rules a real captured packet must satisfy.
/// Mirrors `validate_packet_header` in the Python reference (`capture.py`).
const MIN_PACKET_LEN: u32 = 8;
const MAX_PACKET_LEN: u32 = 2 * 1024 * 1024;

fn validate_header(length: u32, packet_type: u16, padding: u16) -> bool {
    (MIN_PACKET_LEN..=MAX_PACKET_LEN).contains(&length)
        && messages::is_known_packet_type(packet_type)
        && matches!(padding, 0 | 256)
}

/// One outcome of feeding a [`Segment`] into a [`Decoder`]: either a
/// completed packet (decoded, or not -- mirrors `handle_packet` in the
/// Python reference, which logs an unparsed packet and carries on rather
/// than dropping the rest of the stream), or a desync while reassembling.
#[derive(Debug, Clone, PartialEq)]
pub enum DecoderEvent {
    /// A complete packet that decoded successfully.
    Message(Message),
    /// A complete packet whose type or body did not decode.
    DecodeFailed { packet_type: u16, error: DecodeError },
    /// Bytes were dropped while reassembling one of this decoder's streams.
    Desync {
        stream: StreamKey,
        dropped: usize,
        reason: DesyncReason,
    },
}

/// Reassembles captured [`Segment`]s into packets and decodes them, using
/// the game's real header validation and the framing defaults.
pub struct Decoder {
    framer: FramedPacketStreams<StreamKey, fn(u32, u16, u16) -> bool>,
}

impl Decoder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            framer: FramedPacketStreams::with_defaults(validate_header),
        }
    }

    /// Feeds one captured segment, returning every event it produced:
    /// completed packets first (decoded, in emission order), then any
    /// desync events from reassembling this call's stream.
    pub fn feed(&mut self, segment: &Segment) -> Vec<DecoderEvent> {
        let outcome = self.framer.feed_segment(segment);
        let mut events = Vec::with_capacity(outcome.packets.len() + outcome.desyncs.len());

        events.extend(outcome.packets.into_iter().map(|packet| {
            match messages::decode(packet.packet_type, &packet.body) {
                Ok(message) => DecoderEvent::Message(message),
                Err(error) => DecoderEvent::DecodeFailed {
                    packet_type: packet.packet_type,
                    error,
                },
            }
        }));
        events.extend(outcome.desyncs.into_iter().map(|desync| DecoderEvent::Desync {
            stream: desync.stream,
            dropped: desync.dropped,
            reason: desync.reason,
        }));

        events
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}
