//! Everything the app loads at startup: its data folder, settings, item catalog and icons.

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use appdata::{DataDir, Settings};
use game_data::{IconPack, ItemCatalog};
use market::{MarketHistory, WorthModel};

use crate::product;

/// Setting keys the Rust side reads.
pub mod keys {
    /// Closing the window keeps the app running in the tray (default on).
    pub const CLOSE_TO_TRAY: &str = "closeToTray";
    /// Hover values: show an item's value beside the game's tooltip (default on).
    pub const HOVER_VALUES: &str = "hoverValues";
    /// Files taken over from DnDTools on the first run, with the time.
    pub const IMPORTED_FROM_DNDTOOLS: &str = "importedFromDndtools";
    /// Starter market data installed on the first run, with the time.
    pub const STARTER_DATA: &str = "starterData";
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

pub struct AppState {
    pub data: DataDir,
    pub settings: Mutex<Settings>,
    pub catalog: ItemCatalog,
    pub icons: Option<IconPack>,
    /// Market history; None when the database could not be opened.
    pub market: Option<MarketHistory>,
    /// The trained price model; None until one has been trained or imported. Swapped whole when
    /// the model is retrained.
    worth: RwLock<Option<Arc<WorthModel>>>,
    /// The model's median error on held-out listings (a share), when known.
    worth_error: RwLock<Option<f64>>,
    /// Stat pairs that sell for more together, and extra-roll premiums, for pricing.
    learned: RwLock<Arc<crate::training::Learned>>,
    /// A training run is going (one at a time).
    pub training: std::sync::atomic::AtomicBool,
    /// The player's characters and what they own.
    pub characters: Mutex<Vec<state::Character>>,
    /// What the game last showed of the player's Marketplace listings (shared with lister runs).
    pub marketplace: Arc<lister::marketplace_state::MarketplaceState>,
    /// Which merchant is open and what the last sale did (shared with merchant sales).
    pub merchant: Arc<lister::merchant_state::MerchantState>,
    /// Whether live game data is flowing.
    pub live: Mutex<crate::network::LiveStatus>,
    /// Merchant quests and the player's progress; None when the quest data can't be read.
    pub quests: Option<crate::quests_view::Quests>,
    /// The stash sorter: its learning and its run.
    pub sorter: crate::sorter_view::Sorter,
    /// Only one feature drives the mouse at a time; Ctrl+F12 stops it.
    pub input_lock: crate::input_lock::InputLock,
}

pub const MARKET_DB: &str = "market_history.sqlite";
pub const WORTH_MODEL: &str = "worth_model.json";

impl AppState {
    /// Opens the data folder (importing a DnDTools install's market data on the first run), the
    /// settings and the game data under `assets`.
    pub fn load(assets: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let data = DataDir::for_product(product::DATA_FOLDER)?;
        let mut settings = Settings::load(&data.settings_path())?;
        if let Some(dndtools) = DataDir::dndtools_root() {
            match data.import_from_dndtools(&dndtools) {
                Ok(copied) if !copied.is_empty() => {
                    settings.set(keys::IMPORTED_FROM_DNDTOOLS, serde_json::json!({ "files": copied, "at": unix_now() }))?;
                }
                Ok(_) => {}
                // A failed import must not stop the app: it starts without the old data.
                Err(err) => log::warn!("importing DnDTools data failed: {err}"),
            }
        }
        // Without DnDTools, a fresh install starts from the market data that ships with the app.
        match data.install_starter(&assets.join("starter")) {
            Ok(installed) if !installed.is_empty() => {
                settings.set(keys::STARTER_DATA, serde_json::json!({ "files": installed, "at": unix_now() }))?;
            }
            Ok(_) => {}
            Err(err) => log::warn!("installing the starter market data failed: {err}"),
        }
        let catalog = ItemCatalog::load(&assets.join("items.json"))
            .map_err(|err| format!("item data ({}) could not be read: {err}", assets.join("items.json").display()))?;
        let quests = crate::quests_view::Quests::load(assets, &data.data(), &catalog)
            .inspect_err(|err| log::warn!("quests are unavailable: {err}"))
            .ok();
        let icons = match IconPack::open(assets.join("icons.pak")) {
            Ok(icons) => Some(icons),
            Err(err) => {
                log::warn!("icon pack could not be opened, items show without icons: {err}");
                None
            }
        };
        let market = match MarketHistory::open(&data.data().join(MARKET_DB)) {
            Ok(market) => Some(market),
            Err(err) => {
                log::error!("market history could not be opened, market prices are unavailable: {err}");
                None
            }
        };
        let worth_path = data.data().join(WORTH_MODEL);
        let worth = match WorthModel::load(&worth_path) {
            Ok(model) => Some(model),
            Err(err) => {
                if worth_path.exists() {
                    log::warn!("price model could not be read, items show without a value: {err}");
                }
                None
            }
        };
        let saved = crate::stash_view::dndtools_characters_folder()
            .map(|folder| crate::stash_view::load_saved_characters(&folder))
            .unwrap_or_default();
        let characters = crate::stash_view::merge_own_characters(saved, &data.data().join("characters"));
        log::info!("{} characters loaded", characters.len());
        let learned = crate::training::Learned::load(&data.data().join(crate::training::MARKET_MODEL));
        let sorter = crate::sorter_view::Sorter::new(&data.data());
        Ok(Self {
            data,
            settings: Mutex::new(settings),
            catalog,
            icons,
            market,
            worth_error: RwLock::new(crate::training::saved_error(&worth_path)),
            learned: RwLock::new(Arc::new(learned)),
            training: std::sync::atomic::AtomicBool::new(false),
            worth: RwLock::new(worth.map(Arc::new)),
            characters: Mutex::new(characters),
            marketplace: Arc::new(lister::marketplace_state::MarketplaceState::default()),
            merchant: Arc::new(lister::merchant_state::MerchantState::default()),
            live: Mutex::new(crate::network::LiveStatus::default()),
            quests,
            sorter,
            input_lock: crate::input_lock::InputLock::default(),
        })
    }

    /// The current price model, if one has been trained or imported.
    pub fn worth(&self) -> Option<Arc<WorthModel>> {
        self.worth.read().ok().and_then(|model| model.clone())
    }

    /// Puts a newly trained model in use (saving it is the caller's job).
    pub fn set_worth(&self, model: WorthModel, error: Option<f64>) {
        if let Ok(mut current) = self.worth.write() {
            *current = Some(Arc::new(model));
        }
        if let Ok(mut current) = self.worth_error.write() {
            *current = error;
        }
    }

    /// The current model's median error on held-out listings (a share), when known.
    pub fn worth_error(&self) -> Option<f64> {
        self.worth_error.read().ok().and_then(|error| *error)
    }

    /// Puts newly learned roll patterns in use.
    pub fn set_learned(&self, learned: crate::training::Learned) {
        if let Ok(mut current) = self.learned.write() {
            *current = Arc::new(learned);
        }
    }

    /// What pricing learned about roll pairs and extra rolls.
    pub fn learned(&self) -> Arc<crate::training::Learned> {
        self.learned.read().map(|learned| Arc::clone(&learned)).unwrap_or_else(|_| {
            Arc::new(crate::training::Learned { synergies: Default::default(), extra_share: market::EXTRA_ROLL_SHARE })
        })
    }

    pub fn hover_values_on(&self) -> bool {
        self.settings.lock().ok().and_then(|s| s.get::<bool>(keys::HOVER_VALUES)).unwrap_or(true)
    }

    pub fn close_to_tray(&self) -> bool {
        self.settings.lock().ok().and_then(|s| s.get::<bool>(keys::CLOSE_TO_TRAY)).unwrap_or(true)
    }
}
