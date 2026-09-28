//! Turning captured link-layer frames into [`protocol::Segment`]s.
//!
//! Pure, allocation-light functions: given a link type (from `pcap_datalink`) and one captured
//! frame's raw bytes, produce zero or one `Segment`. Anything that isn't IPv4 TCP touching the
//! watched port range is skipped; IPv4 fragments are skipped too (the payload can't be trusted
//! without reassembling the rest first, and the game's small control-plane packets never
//! legitimately fragment).

use std::net::{Ipv4Addr, SocketAddrV4};

use protocol::{Direction, Segment, StreamKey};

use crate::ffi::{DLT_EN10MB, DLT_NULL, DLT_RAW};

/// Inclusive TCP port range the game's lobby server listens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub fn contains(self, port: u16) -> bool {
        (self.start..=self.end).contains(&port)
    }
}

/// The game's default lobby port range.
impl Default for PortRange {
    fn default() -> Self {
        PortRange {
            start: 20200,
            end: 20300,
        }
    }
}

const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERNET_HEADER_LEN: usize = 14;
const PROTO_TCP: u8 = 6;
/// `AF_INET` as Npcap's loopback (`DLT_NULL`) adapter writes it into each frame's 4-byte header.
const NULL_LINK_AF_INET: u32 = 2;

/// Parses one captured frame into a `Segment`, given the adapter's link type (from
/// `pcap_datalink`) and the capture-relative time (seconds, monotonic) to stamp it with.
pub fn parse_frame(
    link_type: i32,
    frame: &[u8],
    port_range: PortRange,
    time: f64,
) -> Option<Segment> {
    let ip_packet = strip_link_header(link_type, frame)?;
    parse_ipv4_tcp(ip_packet, port_range, time)
}

/// Strips the link-layer header, returning the start of the IPv4 packet, or `None` if this is an
/// unrecognized link type, the frame is too short, or it isn't carrying IPv4.
fn strip_link_header(link_type: i32, frame: &[u8]) -> Option<&[u8]> {
    match link_type {
        DLT_EN10MB => {
            if frame.len() < ETHERNET_HEADER_LEN {
                return None;
            }
            let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
            (ethertype == ETHERTYPE_IPV4).then(|| &frame[ETHERNET_HEADER_LEN..])
        }
        DLT_NULL => {
            // 4-byte address-family header, host byte order (BSD/Npcap loopback convention).
            if frame.len() < 4 {
                return None;
            }
            let family = u32::from_ne_bytes([frame[0], frame[1], frame[2], frame[3]]);
            (family == NULL_LINK_AF_INET).then(|| &frame[4..])
        }
        DLT_RAW => Some(frame),
        _ => None,
    }
}

struct Ipv4Header {
    src: Ipv4Addr,
    dst: Ipv4Addr,
    protocol: u8,
    header_len: usize,
    total_len: usize,
    is_fragment: bool,
}

fn parse_ipv4_header(packet: &[u8]) -> Option<Ipv4Header> {
    if packet.len() < 20 {
        return None;
    }
    if packet[0] >> 4 != 4 {
        return None; // not IPv4
    }
    let header_len = ((packet[0] & 0x0f) as usize) * 4;
    if header_len < 20 || packet.len() < header_len {
        return None;
    }
    let total_len = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let flags_and_frag_offset = u16::from_be_bytes([packet[6], packet[7]]);
    let more_fragments = flags_and_frag_offset & 0x2000 != 0;
    let fragment_offset = flags_and_frag_offset & 0x1fff;

    Some(Ipv4Header {
        src: Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]),
        dst: Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]),
        protocol: packet[9],
        header_len,
        total_len,
        is_fragment: more_fragments || fragment_offset != 0,
    })
}

struct TcpHeader {
    src_port: u16,
    dst_port: u16,
    seq: u32,
    data_offset: usize,
    syn: bool,
    fin: bool,
    rst: bool,
}

fn parse_tcp_header(segment: &[u8]) -> Option<TcpHeader> {
    if segment.len() < 20 {
        return None;
    }
    let data_offset = ((segment[12] >> 4) as usize) * 4;
    if data_offset < 20 || segment.len() < data_offset {
        return None;
    }
    let flags = segment[13];

    Some(TcpHeader {
        src_port: u16::from_be_bytes([segment[0], segment[1]]),
        dst_port: u16::from_be_bytes([segment[2], segment[3]]),
        seq: u32::from_be_bytes([segment[4], segment[5], segment[6], segment[7]]),
        data_offset,
        syn: flags & 0x02 != 0,
        fin: flags & 0x01 != 0,
        rst: flags & 0x04 != 0,
    })
}

fn parse_ipv4_tcp(packet: &[u8], port_range: PortRange, time: f64) -> Option<Segment> {
    let ip = parse_ipv4_header(packet)?;
    if ip.protocol != PROTO_TCP || ip.is_fragment {
        return None;
    }

    // `total_len` is the packet length the sender declared; a capture can pad short frames
    // (e.g. Ethernet's 60-byte minimum) or, rarely, snap one shorter than that, so never read
    // past whichever of the two is smaller.
    let ip_end = ip.total_len.min(packet.len());
    if ip_end < ip.header_len {
        return None;
    }
    let tcp_and_payload = &packet[ip.header_len..ip_end];

    let tcp = parse_tcp_header(tcp_and_payload)?;
    let src_in_range = port_range.contains(tcp.src_port);
    let dst_in_range = port_range.contains(tcp.dst_port);
    if !src_in_range && !dst_in_range {
        return None;
    }

    let payload = tcp_and_payload[tcp.data_offset..].to_vec();
    if payload.is_empty() && !(tcp.syn || tcp.fin || tcp.rst) {
        // Stream reassembly only needs empty segments when they carry a connection-lifecycle
        // flag; a bare ACK with no data is dead weight.
        return None;
    }

    Some(Segment {
        stream: StreamKey {
            src: SocketAddrV4::new(ip.src, tcp.src_port),
            dst: SocketAddrV4::new(ip.dst, tcp.dst_port),
        },
        direction: if src_in_range {
            Direction::FromServer
        } else {
            Direction::ToServer
        },
        seq: tcp.seq,
        payload,
        syn: tcp.syn,
        fin: tcp.fin,
        rst: tcp.rst,
        time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETH_HEADER_LEN: usize = ETHERNET_HEADER_LEN;
    const TCP_FLAG_FIN: u8 = 0x01;
    const TCP_FLAG_SYN: u8 = 0x02;
    const TCP_FLAG_RST: u8 = 0x04;
    const TCP_FLAG_PSH: u8 = 0x08;
    const TCP_FLAG_ACK: u8 = 0x10;

    /// Builds hand-crafted Ethernet+IPv4+TCP frames. Checksums are left as zero throughout:
    /// this crate never validates them (real captures can carry unfinalized checksums for
    /// locally-sent packets, due to NIC checksum offload), so tests don't need valid ones either.
    struct FrameBuilder {
        ethertype: u16,
        ip_version_ihl: u8,
        ip_total_len_override: Option<u16>,
        ip_flags_and_frag_offset: u16,
        protocol: u8,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        ip_options: Vec<u8>,
        src_port: u16,
        dst_port: u16,
        seq: u32,
        tcp_flags: u8,
        tcp_data_offset_words: u8,
        tcp_options: Vec<u8>,
        payload: Vec<u8>,
    }

    impl FrameBuilder {
        fn new() -> Self {
            FrameBuilder {
                ethertype: ETHERTYPE_IPV4,
                ip_version_ihl: 0x45,
                ip_total_len_override: None,
                ip_flags_and_frag_offset: 0,
                protocol: PROTO_TCP,
                src_ip: [10, 0, 0, 1],
                dst_ip: [10, 0, 0, 2],
                ip_options: Vec::new(),
                src_port: 20250, // in the default game port range
                dst_port: 51000, // an ordinary ephemeral client port
                seq: 1000,
                tcp_flags: TCP_FLAG_PSH | TCP_FLAG_ACK,
                tcp_data_offset_words: 5,
                tcp_options: Vec::new(),
                payload: Vec::new(),
            }
        }

        fn payload(mut self, bytes: &[u8]) -> Self {
            self.payload = bytes.to_vec();
            self
        }

        fn ports(mut self, src: u16, dst: u16) -> Self {
            self.src_port = src;
            self.dst_port = dst;
            self
        }

        fn flags(mut self, flags: u8) -> Self {
            self.tcp_flags = flags;
            self
        }

        fn protocol(mut self, protocol: u8) -> Self {
            self.protocol = protocol;
            self
        }

        fn ethertype(mut self, ethertype: u16) -> Self {
            self.ethertype = ethertype;
            self
        }

        /// Adds 4 bytes of IPv4 options (a no-op "NOP NOP NOP NOP" pattern) and bumps the IHL.
        fn with_ip_options(mut self) -> Self {
            self.ip_options = vec![0x01, 0x01, 0x01, 0x01];
            self.ip_version_ihl = 0x40 | (5 + (self.ip_options.len() / 4)) as u8;
            self
        }

        fn fragment(mut self, more_fragments: bool, fragment_offset: u16) -> Self {
            let mut value = fragment_offset & 0x1fff;
            if more_fragments {
                value |= 0x2000;
            }
            self.ip_flags_and_frag_offset = value;
            self
        }

        fn override_total_len(mut self, len: u16) -> Self {
            self.ip_total_len_override = Some(len);
            self
        }

        /// Builds the IPv4 header followed by the TCP header and payload (no link-layer header).
        fn build_ip_and_tcp(&self) -> Vec<u8> {
            let ip_header_len = (self.ip_version_ihl & 0x0f) as usize * 4;
            let tcp_header_len = self.tcp_data_offset_words as usize * 4;
            let total_len = self
                .ip_total_len_override
                .unwrap_or((ip_header_len + tcp_header_len + self.payload.len()) as u16);

            let mut buf = Vec::new();
            buf.push(self.ip_version_ihl);
            buf.push(0); // DSCP/ECN, unused by the parser
            buf.extend_from_slice(&total_len.to_be_bytes());
            buf.extend_from_slice(&0u16.to_be_bytes()); // identification, unused
            buf.extend_from_slice(&self.ip_flags_and_frag_offset.to_be_bytes());
            buf.push(64); // TTL, unused
            buf.push(self.protocol);
            buf.extend_from_slice(&0u16.to_be_bytes()); // header checksum, not validated
            buf.extend_from_slice(&self.src_ip);
            buf.extend_from_slice(&self.dst_ip);
            buf.extend_from_slice(&self.ip_options);
            assert_eq!(buf.len(), ip_header_len, "test bug: IHL doesn't match bytes written");

            buf.extend_from_slice(&self.src_port.to_be_bytes());
            buf.extend_from_slice(&self.dst_port.to_be_bytes());
            buf.extend_from_slice(&self.seq.to_be_bytes());
            buf.extend_from_slice(&0u32.to_be_bytes()); // ack number, unused
            buf.push(self.tcp_data_offset_words << 4);
            buf.push(self.tcp_flags);
            buf.extend_from_slice(&0xffffu16.to_be_bytes()); // window, unused
            buf.extend_from_slice(&0u16.to_be_bytes()); // checksum, not validated
            buf.extend_from_slice(&0u16.to_be_bytes()); // urgent pointer, unused
            buf.extend_from_slice(&self.tcp_options);
            buf.extend_from_slice(&self.payload);
            buf
        }

        /// Builds a full Ethernet II frame (arbitrary MACs; the parser ignores them).
        fn build_ethernet(&self) -> Vec<u8> {
            let mut frame = vec![0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb];
            frame.extend_from_slice(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]);
            frame.extend_from_slice(&self.ethertype.to_be_bytes());
            frame.extend_from_slice(&self.build_ip_and_tcp());
            frame
        }

        /// Builds a Npcap loopback (`DLT_NULL`) frame: a 4-byte address-family header, then IP.
        fn build_null_loopback(&self) -> Vec<u8> {
            let mut frame = NULL_LINK_AF_INET.to_ne_bytes().to_vec();
            frame.extend_from_slice(&self.build_ip_and_tcp());
            frame
        }
    }

    fn server_addr() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 20250)
    }

    fn client_addr() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 51000)
    }

    #[test]
    fn valid_tcp_with_payload_is_kept() {
        // Arrange: a server reply (src port in range) carrying a payload.
        let frame = FrameBuilder::new().payload(b"hello").build_ethernet();

        // Act
        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 42.5).unwrap();

        // Assert
        assert_eq!(segment.stream.src, server_addr());
        assert_eq!(segment.stream.dst, client_addr());
        assert_eq!(segment.direction, Direction::FromServer);
        assert_eq!(segment.seq, 1000);
        assert_eq!(segment.payload, b"hello");
        assert!(!segment.syn && !segment.fin && !segment.rst);
        assert_eq!(segment.time, 42.5);
    }

    #[test]
    fn syn_without_payload_is_kept() {
        let frame = FrameBuilder::new().flags(TCP_FLAG_SYN).build_ethernet();

        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).unwrap();

        assert!(segment.syn);
        assert!(segment.payload.is_empty());
    }

    #[test]
    fn fin_without_payload_is_kept() {
        let frame = FrameBuilder::new()
            .flags(TCP_FLAG_FIN | TCP_FLAG_ACK)
            .build_ethernet();

        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).unwrap();

        assert!(segment.fin);
        assert!(segment.payload.is_empty());
    }

    #[test]
    fn rst_without_payload_is_kept() {
        let frame = FrameBuilder::new().flags(TCP_FLAG_RST).build_ethernet();

        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).unwrap();

        assert!(segment.rst);
        assert!(segment.payload.is_empty());
    }

    #[test]
    fn ack_only_without_payload_is_dropped() {
        // A bare ACK carries nothing reassembly needs.
        let frame = FrameBuilder::new().flags(TCP_FLAG_ACK).build_ethernet();

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn ipv4_with_options_is_parsed_correctly() {
        let frame = FrameBuilder::new()
            .with_ip_options()
            .payload(b"past-the-options")
            .build_ethernet();

        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).unwrap();

        assert_eq!(segment.payload, b"past-the-options");
    }

    #[test]
    fn non_tcp_protocol_is_dropped() {
        const PROTO_UDP: u8 = 17;
        let frame = FrameBuilder::new().protocol(PROTO_UDP).build_ethernet();

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn wrong_port_is_dropped() {
        // Neither port falls in the watched range.
        let frame = FrameBuilder::new().ports(4000, 4001).build_ethernet();

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn direction_is_to_server_when_destination_port_is_in_range() {
        let frame = FrameBuilder::new()
            .ports(51000, 20250)
            .payload(b"client-request")
            .build_ethernet();

        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).unwrap();

        assert_eq!(segment.direction, Direction::ToServer);
    }

    #[test]
    fn fragment_with_nonzero_offset_is_dropped() {
        // A later fragment of some larger datagram: no TCP header to trust here at all.
        let frame = FrameBuilder::new()
            .fragment(false, 185)
            .payload(b"fragment-tail")
            .build_ethernet();

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn fragment_with_more_fragments_flag_is_dropped() {
        // The first fragment of a datagram that continues: still not safe to trust in isolation.
        let frame = FrameBuilder::new().fragment(true, 0).build_ethernet();

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn truncated_ethernet_header_is_dropped() {
        let frame = vec![0xaa; 10]; // shorter than the 14-byte Ethernet header

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn truncated_ip_header_is_dropped() {
        let mut frame = FrameBuilder::new().build_ethernet();
        frame.truncate(ETH_HEADER_LEN + 10); // shorter than the 20-byte minimum IPv4 header

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn truncated_tcp_header_is_dropped() {
        let mut frame = FrameBuilder::new().payload(b"unreachable").build_ethernet();
        frame.truncate(ETH_HEADER_LEN + 20 + 10); // full IP header, only 10 bytes of TCP header

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn non_ipv4_ethertype_is_dropped() {
        const ETHERTYPE_ARP: u16 = 0x0806;
        let frame = FrameBuilder::new().ethertype(ETHERTYPE_ARP).build_ethernet();

        assert!(parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn unknown_link_type_is_dropped() {
        let frame = FrameBuilder::new().build_ethernet();
        const SOME_UNHANDLED_DLT: i32 = 999;

        assert!(parse_frame(SOME_UNHANDLED_DLT, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn dlt_null_loopback_is_parsed() {
        let frame = FrameBuilder::new().payload(b"loopback").build_null_loopback();

        let segment = parse_frame(DLT_NULL, &frame, PortRange::default(), 0.0).unwrap();

        assert_eq!(segment.payload, b"loopback");
    }

    #[test]
    fn dlt_null_wrong_address_family_is_dropped() {
        const NOT_AF_INET: u32 = 99;
        let mut frame = NOT_AF_INET.to_ne_bytes().to_vec();
        frame.extend_from_slice(&FrameBuilder::new().build_ip_and_tcp());

        assert!(parse_frame(DLT_NULL, &frame, PortRange::default(), 0.0).is_none());
    }

    #[test]
    fn dlt_raw_is_parsed_directly() {
        let frame = FrameBuilder::new().payload(b"raw-ip").build_ip_and_tcp();

        let segment = parse_frame(DLT_RAW, &frame, PortRange::default(), 0.0).unwrap();

        assert_eq!(segment.payload, b"raw-ip");
    }

    #[test]
    fn ip_total_len_shorter_than_capture_is_respected() {
        // Simulates a snap length or padded short frame: extra captured bytes past the IP
        // header's own declared length must not leak into the payload.
        let mut builder = FrameBuilder::new().payload(b"real");
        let real_len = builder.build_ip_and_tcp().len() as u16;
        builder = builder.override_total_len(real_len);
        let mut frame = builder.build_ethernet();
        frame.extend_from_slice(b"junk-from-ethernet-padding");

        let segment = parse_frame(DLT_EN10MB, &frame, PortRange::default(), 0.0).unwrap();

        assert_eq!(segment.payload, b"real");
    }
}
