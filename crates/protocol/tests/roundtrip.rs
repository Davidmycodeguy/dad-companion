//! Round-trip tests: build a real message with the generated types, encode
//! it behind a correct 8-byte header, split it across several segments of
//! one TCP stream -- fed out of order -- and check [`Decoder`] reassembles
//! and decodes it back to the same message, under the right name.

use std::net::{Ipv4Addr, SocketAddrV4};

use prost::Message as _;
use protocol::messages::proto::{Ss2cInventoryInfoRes, Ss2cMarketplaceItemListRes, SmarketplaceItemInfo};
use protocol::messages::Decoded;
use protocol::{Decoder, DecoderEvent, Direction, Segment, StreamKey};

/// `S2C_MARKETPLACE_ITEM_LIST_RES`'s `PacketCommand` value.
const PT_MARKETPLACE_ITEM_LIST_RES: u16 = 3512;
/// `S2C_INVENTORY_INFO_RES`'s `PacketCommand` value.
const PT_INVENTORY_INFO_RES: u16 = 502;

fn stream_key() -> StreamKey {
    StreamKey {
        src: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 20_201),
        dst: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 51_000),
    }
}

/// Packs `body` behind a correct 8-byte `<IHH` header: length includes the
/// header, no padding.
fn framed(packet_type: u16, body: &[u8]) -> Vec<u8> {
    let length = u32::try_from(body.len() + 8).expect("test body fits in a u32 length");
    let mut bytes = Vec::with_capacity(body.len() + 8);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(&packet_type.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn segment(stream: StreamKey, seq: u32, payload: &[u8]) -> Segment {
    Segment {
        stream,
        direction: Direction::FromServer,
        seq,
        payload: payload.to_vec(),
        syn: false,
        fin: false,
        rst: false,
        time: 0.0,
    }
}

#[test]
fn marketplace_item_list_decodes_after_out_of_order_reassembly() {
    let message = Ss2cMarketplaceItemListRes {
        item_infos: vec![SmarketplaceItemInfo {
            listing_id: 42,
            item: None,
            price: 1_000,
            remain_expiration_time: 3_600,
            nickname: None,
        }],
        current_page: 1,
        max_page: 3,
    };
    let frame = framed(PT_MARKETPLACE_ITEM_LIST_RES, &message.encode_to_vec());
    assert!(
        frame.len() >= 12,
        "test frame must be long enough to split into three non-trivial chunks"
    );

    let stream = stream_key();
    let base_seq = 10_000u32;
    let first_end = 5;
    let second_end = frame.len() - 3;
    let chunk_1 = &frame[..first_end];
    let chunk_2 = &frame[first_end..second_end];
    let chunk_3 = &frame[second_end..];

    let mut decoder = Decoder::new();

    // Chunk 1 establishes the stream's baseline sequence -- it must arrive
    // "first" (in call order) for reassembly to know where the stream
    // starts; see `Decoder::feed`'s and `feed_segment`'s documentation for
    // why a real capture cannot mark an earlier segment as out-of-order
    // after the fact. Chunks 3 and 2 then arrive out of order relative to
    // each other, exercising the pending/gap-fill path.
    let events_1 = decoder.feed(&segment(stream, base_seq, chunk_1));
    assert!(events_1.is_empty(), "an incomplete frame decodes nothing yet");

    let events_3 = decoder.feed(&segment(
        stream,
        base_seq + u32::try_from(first_end + chunk_2.len()).unwrap(),
        chunk_3,
    ));
    assert!(events_3.is_empty(), "still waiting on the gap chunk_2 fills");

    let mut events_2 = decoder.feed(&segment(stream, base_seq + u32::try_from(first_end).unwrap(), chunk_2));
    assert_eq!(events_2.len(), 1, "the completed frame decodes exactly once");

    let decoded = match events_2.remove(0) {
        DecoderEvent::Message(message) => message,
        other => panic!("expected a decoded message, got {other:?}"),
    };
    assert_eq!(decoded.packet_type, PT_MARKETPLACE_ITEM_LIST_RES);
    assert_eq!(decoded.name, "S2C_MARKETPLACE_ITEM_LIST_RES");
    assert_eq!(decoded.decoded, Decoded::MarketplaceItemList(message));
}

#[test]
fn inventory_info_decodes_from_a_single_segment() {
    let message = Ss2cInventoryInfoRes {
        result: 1,
        inventory_items: Vec::new(),
    };
    let frame = framed(PT_INVENTORY_INFO_RES, &message.encode_to_vec());
    let stream = stream_key();

    let mut decoder = Decoder::new();
    let mut events = decoder.feed(&segment(stream, 500, &frame));

    assert_eq!(events.len(), 1);
    let decoded = match events.remove(0) {
        DecoderEvent::Message(message) => message,
        other => panic!("expected a decoded message, got {other:?}"),
    };
    assert_eq!(decoded.name, "S2C_INVENTORY_INFO_RES");
    assert_eq!(decoded.decoded, Decoded::InventoryInfo(message));
}
