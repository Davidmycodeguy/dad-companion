//! Generated protobuf types, and decoding a framed packet's body into them.
//!
//! Mirrors the relevant parts of the Python reference's `capture.py`:
//! `_build_proto_map` (packet type -> message type), `validate_packet_header`
//! (which packet types are legal at all) and `_parse_proto_with_error`
//! (decoding a body once its type is known).

/// Rust types generated from the game's `.proto` files by `build.rs`
/// (`protox` parses them, `prost-build` generates the types). Every message
/// and enum the game defines is public here under its generated name, so
/// callers are never limited to the handful of types [`Decoded`] names.
pub mod proto {
    #![allow(clippy::all, missing_docs, non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/dc.packet.rs"));

    /// `DC.Packet.Defines`: constant-holding messages, unused by any other
    /// packet's fields (nothing imports `_Defins.proto`) but compiled for
    /// completeness.
    pub mod defines {
        #![allow(clippy::all, missing_docs, non_snake_case)]
        include!(concat!(env!("OUT_DIR"), "/dc.packet.defines.rs"));
    }
}

// `resolve_message_name` and `MAPPED_PACKET_COUNT`: built by build.rs from
// the compiled descriptors, mirroring `_build_proto_map`'s naming
// convention. See that function's doc comment in build.rs for the algorithm.
include!(concat!(env!("OUT_DIR"), "/packet_message_map.rs"));

/// The raw `FileDescriptorSet` for the compiled `.proto` files. Not used by
/// this crate; kept so a future consumer can do name-based reflection (e.g.
/// via `prost-reflect`) without re-running protox.
pub static FILE_DESCRIPTOR_SET_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

use proto::PacketCommand;
use prost::Message as _;

/// Returns whether `packet_type` is a value of the game's `PacketCommand`
/// enum. Used for header validation: mirrors `validate_packet_header`'s
/// `proto_type in _PACKET_COMMAND_VALUES` check in the Python reference,
/// which likewise accepts every enum value (including the `MIN_`/`MAX_`
/// range markers), not only the ones with a mapped message.
#[must_use]
pub fn is_known_packet_type(packet_type: u16) -> bool {
    PacketCommand::is_valid(i32::from(packet_type))
}

/// How many `PacketCommand` values have a message mapped to them (mirrors
/// the size Python's `_build_proto_map` would build).
#[must_use]
pub fn mapped_packet_type_count() -> usize {
    MAPPED_PACKET_COUNT
}

/// A successfully decoded packet: its type, the `PacketCommand` name (e.g.
/// `"S2C_MARKETPLACE_ITEM_LIST_RES"`), and the decoded payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub packet_type: u16,
    pub name: &'static str,
    pub decoded: Decoded,
}

/// A decoded packet body.
///
/// The named variants are the ones `handle_packet`'s callers in the Python
/// reference actually consume (marketplace item lists, my listings,
/// merchant stock, inventory/stash info, quests, character info). Every
/// other packet type that has a mapped message still decodes successfully
/// as [`Decoded::Other`], carrying its raw body and resolved message name
/// so a caller can parse it with the matching type from [`proto`] directly
/// -- no message the game defines is unreachable, only the ergonomic
/// shortcut is limited to these variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    /// `S2C_MARKETPLACE_ITEM_LIST_RES`.
    MarketplaceItemList(proto::Ss2cMarketplaceItemListRes),
    /// `S2C_MARKETPLACE_MY_ITEM_LIST_RES`.
    MarketplaceMyItemList(proto::Ss2cMarketplaceMyItemListRes),
    /// `S2C_MERCHANT_STOCK_BUY_ITEM_LIST_RES`.
    MerchantStock(proto::Ss2cMerchantStockBuyItemListRes),
    /// `S2C_INVENTORY_INFO_RES` (inventory/stash contents).
    InventoryInfo(proto::Ss2cInventoryInfoRes),
    /// `S2C_MERCHANT_QUEST_LIST_INFO_RES`.
    MerchantQuestList(proto::Ss2cMerchantQuestListInfoRes),
    /// `S2C_MERCHANT_QUEST_LOG_LIST_RES`.
    MerchantQuestLogList(proto::Ss2cMerchantQuestLogListRes),
    /// `S2C_MERCHANT_LIST_RES`: the merchants and whether each is unlocked.
    MerchantList(proto::Ss2cMerchantListRes),
    /// `S2C_MERCHANT_STOCK_SELL_BACK_RES`: the answer to Make Deal on a merchant's Sell tab.
    MerchantSellBack(proto::Ss2cMerchantStockSellBackRes),
    /// `S2C_MERCHANT_QUEST_SELECT_RES`: a quest was accepted.
    MerchantQuestSelect(proto::Ss2cMerchantQuestSelectRes),
    /// `S2C_MERCHANT_QUEST_COMPLETE_RES`: a quest was turned in.
    MerchantQuestComplete(proto::Ss2cMerchantQuestCompleteRes),
    /// `S2C_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES`: items handed in toward a quest.
    MerchantQuestContentValueStack(proto::Ss2cMerchantQuestContentValueStackRes),
    /// `S2C_MARKETPLACE_ITEM_REGISTER_RES`: the answer to listing an item.
    MarketplaceRegister(proto::Ss2cMarketplaceItemRegisterRes),
    /// `S2C_MARKETPLACE_TRANSFER_ITEMS_RES`: the answer to collecting sold or expired listings.
    MarketplaceTransfer(proto::Ss2cMarketplaceTransferItemsRes),
    /// `S2C_MARKETPLACE_ITEM_HAS_SOLD_NOT`: one of the player's listings sold.
    MarketplaceItemSold(proto::Ss2cMarketplaceItemHasSoldNot),
    /// `S2C_LOBBY_CHARACTER_INFO_RES`. Boxed: at 448+ bytes it otherwise
    /// makes every `Decoded` value that size, even the empty ones.
    CharacterInfo(Box<proto::Ss2cLobbyCharacterInfoRes>),
    /// A packet type with a mapped message but no dedicated variant above.
    Other {
        /// The mapped message's generated-source name (e.g.
        /// `"SS2C_ALIVE_RES"`), for looking up the matching type in
        /// [`proto`].
        message_name: &'static str,
        /// The packet's undecoded body (after the 8-byte header).
        body: Vec<u8>,
    },
}

/// Why [`decode`] could not produce a [`Message`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecodeError {
    /// `packet_type` is not a value of the `PacketCommand` enum at all.
    #[error("packet type {0} is not a known PacketCommand value")]
    UnknownPacketType(u16),
    /// `packet_type` is a real `PacketCommand` value, but no generated
    /// message is mapped to it (mirrors `_parse_proto_with_error`'s "No
    /// generated protobuf class is mapped to this packet type").
    #[error("no generated protobuf message is mapped to packet type {packet_type} ({name})")]
    NoMessageMapped {
        packet_type: u16,
        name: &'static str,
    },
    /// The body did not parse as the message mapped to `packet_type`.
    #[error("failed to decode {name} (packet type {packet_type}): {source}")]
    Malformed {
        packet_type: u16,
        name: &'static str,
        #[source]
        source: prost::DecodeError,
    },
}

/// Decodes a framed packet's body (bytes after the 8-byte header) into a
/// [`Message`], using `packet_type` to look up which generated message type
/// to parse it as.
pub fn decode(packet_type: u16, body: &[u8]) -> Result<Message, DecodeError> {
    let command = PacketCommand::try_from(i32::from(packet_type))
        .map_err(|_unknown| DecodeError::UnknownPacketType(packet_type))?;
    let name = command.as_str_name();

    let message_name = resolve_message_name(i32::from(packet_type))
        .ok_or(DecodeError::NoMessageMapped { packet_type, name })?;

    let decoded = decode_known(packet_type, name, message_name, body)?;
    Ok(Message {
        packet_type,
        name,
        decoded,
    })
}

// `PacketCommand` values for the curated `Decoded` variants. Named (rather
// than matched via the generated enum's Rust identifiers) so this reads
// against the same `S2C_..._RES`-style names used throughout the .proto
// files and the Python reference, independent of prost's identifier casing.
/// `S2C_LOBBY_CHARACTER_INFO_RES`.
const PT_LOBBY_CHARACTER_INFO_RES: u16 = 44;
/// `S2C_INVENTORY_INFO_RES`.
const PT_INVENTORY_INFO_RES: u16 = 502;
/// `S2C_MERCHANT_STOCK_BUY_ITEM_LIST_RES`.
const PT_MERCHANT_STOCK_BUY_ITEM_LIST_RES: u16 = 1354;
/// `S2C_MERCHANT_QUEST_LIST_INFO_RES`.
const PT_MERCHANT_QUEST_LIST_INFO_RES: u16 = 1401;
/// `S2C_MERCHANT_QUEST_LOG_LIST_RES`.
const PT_MERCHANT_QUEST_LOG_LIST_RES: u16 = 1481;
/// `S2C_MARKETPLACE_ITEM_LIST_RES`.
const PT_MARKETPLACE_ITEM_LIST_RES: u16 = 3512;
/// `S2C_MARKETPLACE_MY_ITEM_LIST_RES`.
const PT_MARKETPLACE_MY_ITEM_LIST_RES: u16 = 3514;
/// `S2C_MERCHANT_LIST_RES`.
const PT_MERCHANT_LIST_RES: u16 = 1352;
/// `S2C_MERCHANT_STOCK_SELL_BACK_RES`.
const PT_MERCHANT_STOCK_SELL_BACK_RES: u16 = 1360;
/// `S2C_MERCHANT_QUEST_SELECT_RES`.
const PT_MERCHANT_QUEST_SELECT_RES: u16 = 1403;
/// `S2C_MERCHANT_QUEST_COMPLETE_RES`.
const PT_MERCHANT_QUEST_COMPLETE_RES: u16 = 1405;
/// `S2C_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES`.
const PT_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES: u16 = 1407;
/// `S2C_MARKETPLACE_ITEM_REGISTER_RES`.
const PT_MARKETPLACE_ITEM_REGISTER_RES: u16 = 3534;
/// `S2C_MARKETPLACE_TRANSFER_ITEMS_RES`.
const PT_MARKETPLACE_TRANSFER_ITEMS_RES: u16 = 3552;
/// `S2C_MARKETPLACE_ITEM_HAS_SOLD_NOT`.
const PT_MARKETPLACE_ITEM_HAS_SOLD_NOT: u16 = 3560;

/// Parses `body` as the message mapped to `packet_type`, either into one of
/// [`Decoded`]'s named variants or, for every other mapped type, into
/// [`Decoded::Other`].
fn decode_known(
    packet_type: u16,
    name: &'static str,
    message_name: &'static str,
    body: &[u8],
) -> Result<Decoded, DecodeError> {
    let malformed = |source| DecodeError::Malformed {
        packet_type,
        name,
        source,
    };
    Ok(match packet_type {
        PT_MARKETPLACE_ITEM_LIST_RES => Decoded::MarketplaceItemList(
            proto::Ss2cMarketplaceItemListRes::decode(body).map_err(malformed)?,
        ),
        PT_MARKETPLACE_MY_ITEM_LIST_RES => Decoded::MarketplaceMyItemList(
            proto::Ss2cMarketplaceMyItemListRes::decode(body).map_err(malformed)?,
        ),
        PT_MERCHANT_STOCK_BUY_ITEM_LIST_RES => Decoded::MerchantStock(
            proto::Ss2cMerchantStockBuyItemListRes::decode(body).map_err(malformed)?,
        ),
        PT_INVENTORY_INFO_RES => {
            Decoded::InventoryInfo(proto::Ss2cInventoryInfoRes::decode(body).map_err(malformed)?)
        }
        PT_MERCHANT_QUEST_LIST_INFO_RES => Decoded::MerchantQuestList(
            proto::Ss2cMerchantQuestListInfoRes::decode(body).map_err(malformed)?,
        ),
        PT_MERCHANT_QUEST_LOG_LIST_RES => Decoded::MerchantQuestLogList(
            proto::Ss2cMerchantQuestLogListRes::decode(body).map_err(malformed)?,
        ),
        PT_MERCHANT_LIST_RES => Decoded::MerchantList(proto::Ss2cMerchantListRes::decode(body).map_err(malformed)?),
        PT_MERCHANT_STOCK_SELL_BACK_RES => {
            Decoded::MerchantSellBack(proto::Ss2cMerchantStockSellBackRes::decode(body).map_err(malformed)?)
        }
        PT_MERCHANT_QUEST_SELECT_RES => {
            Decoded::MerchantQuestSelect(proto::Ss2cMerchantQuestSelectRes::decode(body).map_err(malformed)?)
        }
        PT_MERCHANT_QUEST_COMPLETE_RES => {
            Decoded::MerchantQuestComplete(proto::Ss2cMerchantQuestCompleteRes::decode(body).map_err(malformed)?)
        }
        PT_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES => Decoded::MerchantQuestContentValueStack(
            proto::Ss2cMerchantQuestContentValueStackRes::decode(body).map_err(malformed)?,
        ),
        PT_MARKETPLACE_ITEM_REGISTER_RES => {
            Decoded::MarketplaceRegister(proto::Ss2cMarketplaceItemRegisterRes::decode(body).map_err(malformed)?)
        }
        PT_MARKETPLACE_TRANSFER_ITEMS_RES => {
            Decoded::MarketplaceTransfer(proto::Ss2cMarketplaceTransferItemsRes::decode(body).map_err(malformed)?)
        }
        PT_MARKETPLACE_ITEM_HAS_SOLD_NOT => {
            Decoded::MarketplaceItemSold(proto::Ss2cMarketplaceItemHasSoldNot::decode(body).map_err(malformed)?)
        }
        PT_LOBBY_CHARACTER_INFO_RES => Decoded::CharacterInfo(Box::new(
            proto::Ss2cLobbyCharacterInfoRes::decode(body).map_err(malformed)?,
        )),
        _ => Decoded::Other {
            message_name,
            body: body.to_vec(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_packet_command_value_is_known() {
        assert!(is_known_packet_type(PT_MARKETPLACE_ITEM_LIST_RES));
        assert!(is_known_packet_type(0)); // PACKET_NONE is still a valid value.
        assert!(!is_known_packet_type(u16::MAX));
    }

    #[test]
    fn decode_rejects_a_packet_type_outside_the_enum() {
        let error = decode(u16::MAX, &[]).unwrap_err();
        assert!(matches!(error, DecodeError::UnknownPacketType(v) if v == u16::MAX));
    }

    #[test]
    fn decode_reports_a_known_value_with_no_mapped_message() {
        // PACKET_NONE is a real enum value but a pure range marker: skipped
        // by `_build_proto_map`, so no message is ever mapped to it.
        let error = decode(0, &[]).unwrap_err();
        assert!(matches!(
            error,
            DecodeError::NoMessageMapped { packet_type: 0, name: "PACKET_NONE" }
        ));
    }

    #[test]
    fn decode_reports_malformed_bodies() {
        let error = decode(PT_MARKETPLACE_ITEM_LIST_RES, &[0xFF]).unwrap_err();
        assert!(matches!(error, DecodeError::Malformed { packet_type: PT_MARKETPLACE_ITEM_LIST_RES, .. }));
    }

    #[test]
    fn a_mapped_but_uncurated_type_decodes_as_other() {
        // S2C_ALIVE_RES: mapped (SS2C_ALIVE_RES exists and is empty) but has
        // no dedicated `Decoded` variant.
        let message = decode(2, &[]).unwrap();
        assert_eq!(message.name, "S2C_ALIVE_RES");
        assert!(matches!(
            message.decoded,
            Decoded::Other { message_name: "SS2C_ALIVE_RES", ref body } if body.is_empty()
        ));
    }

    #[test]
    fn most_packet_command_values_have_a_mapped_message() {
        // Parity check against the Python reference's `_build_proto_map`,
        // which maps the large majority of real (non range-marker) values.
        assert!(mapped_packet_type_count() > 500);
    }
}

#[cfg(test)]
mod lister_and_quest_messages {
    use super::*;

    #[test]
    fn register_and_sell_back_answers_decode_to_their_variants() {
        let body = proto::Ss2cMarketplaceItemRegisterRes { result: 1 }.encode_to_vec();
        assert!(matches!(decode(PT_MARKETPLACE_ITEM_REGISTER_RES, &body).unwrap().decoded, Decoded::MarketplaceRegister(r) if r.result == 1));
        let body = proto::Ss2cMarketplaceTransferItemsRes { result: 1 }.encode_to_vec();
        assert!(matches!(decode(PT_MARKETPLACE_TRANSFER_ITEMS_RES, &body).unwrap().decoded, Decoded::MarketplaceTransfer(_)));
        let body = proto::Ss2cMarketplaceItemHasSoldNot { is_sold: 1 }.encode_to_vec();
        assert!(matches!(decode(PT_MARKETPLACE_ITEM_HAS_SOLD_NOT, &body).unwrap().decoded, Decoded::MarketplaceItemSold(_)));
        let body = proto::Ss2cMerchantStockSellBackRes::default().encode_to_vec();
        assert!(matches!(decode(PT_MERCHANT_STOCK_SELL_BACK_RES, &body).unwrap().decoded, Decoded::MerchantSellBack(_)));
    }

    #[test]
    fn quest_messages_decode_to_their_variants() {
        let cases: [(u16, Vec<u8>); 4] = [
            (PT_MERCHANT_LIST_RES, proto::Ss2cMerchantListRes::default().encode_to_vec()),
            (PT_MERCHANT_QUEST_SELECT_RES, proto::Ss2cMerchantQuestSelectRes::default().encode_to_vec()),
            (PT_MERCHANT_QUEST_COMPLETE_RES, proto::Ss2cMerchantQuestCompleteRes::default().encode_to_vec()),
            (PT_MERCHANT_QUEST_CONTENT_VALUE_STACK_RES, proto::Ss2cMerchantQuestContentValueStackRes::default().encode_to_vec()),
        ];
        for (packet_type, body) in cases {
            let decoded = decode(packet_type, &body).unwrap().decoded;
            assert!(!matches!(decoded, Decoded::Other { .. }), "{packet_type} fell through to Other");
        }
    }
}
