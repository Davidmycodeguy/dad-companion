//! What the player owns: their characters and the items in each stash, as the game's lobby
//! messages report them. Plain data shared by the sorter, the auto lister and the Stash page;
//! turning decoded messages into it lives next to the decoder.

pub mod import;
pub mod stash;

pub use stash::{grid_size, is_off_limits, slot_cell, Character, OwnedItem, LOCKED_SEASONAL_STASH};
