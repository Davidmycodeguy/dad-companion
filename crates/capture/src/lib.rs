//! Passive capture of the game's lobby TCP traffic via Npcap.
//!
//! This crate only ever reads traffic: it never injects, replays or otherwise sends packets.
//! It talks to `wpcap.dll` directly (loaded at runtime, no Npcap SDK / `.lib` required) instead
//! of shelling out to `tshark`, so end users only need the Npcap driver installed.

mod capture;
mod device;
mod error;
mod ffi;
mod handle;
mod interface;
mod library;
mod parse;

pub use capture::{list_adapters, Capture, Config, CounterSnapshot};
pub use device::DeviceInfo;
pub use error::Error;
pub use parse::PortRange;
