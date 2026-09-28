//! Dark and Darker game data shipped with the app: the item catalog (`assets/items.json`) and the
//! icon pack (`assets/icons.pak`).

pub mod icons;
pub mod items;
pub mod stats;

pub use icons::{canonical_icon_path, IconPack};
pub use items::{Item, ItemCatalog, Rarity, SearchHit};
pub use stats::{is_percent_stat, roll_text, stat_label};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not read game data: {0}")]
    Io(#[from] std::io::Error),
    #[error("item data is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("icon pack is damaged: {0}")]
    Zip(#[from] zip::result::ZipError),
}
