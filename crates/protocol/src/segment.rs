//! A captured TCP segment of the game's traffic: the data the `capture` crate hands over.

use std::net::SocketAddrV4;

/// One direction of one TCP connection, from `src` to `dst`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamKey {
    pub src: SocketAddrV4,
    pub dst: SocketAddrV4,
}

/// Which way a segment travels relative to the game server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// From the game server to the player's PC (replies like S2C_...).
    FromServer,
    /// From the player's PC to the game server (requests like C2S_...).
    ToServer,
}

/// A TCP segment's payload with what reassembly needs: its connection and direction, sequence
/// number and flags, and when it was captured (seconds on a monotonic clock).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub stream: StreamKey,
    pub direction: Direction,
    /// TCP sequence number of the first payload byte.
    pub seq: u32,
    pub payload: Vec<u8>,
    pub syn: bool,
    pub fin: bool,
    pub rst: bool,
    /// Capture time in seconds (monotonic, arbitrary origin).
    pub time: f64,
}
