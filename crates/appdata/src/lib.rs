//! Where the app keeps its data (`%LOCALAPPDATA%\<product>\`), the first-run import of a DnDTools
//! install's market data and trained models, and the settings file.

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings are not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not copy the market database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("LOCALAPPDATA is not set")]
    NoAppData,
}

/// Files taken over from a DnDTools install on first run (never over our own copies).
const IMPORTED: [&str; 3] = ["market_history.sqlite", "worth_model.json", "market_model.json"];
const MARKET_DB: &str = "market_history.sqlite";

/// The app's data folder: settings at the top, data files in `data/`.
#[derive(Debug, Clone)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    /// `%LOCALAPPDATA%\<folder>`, created if needed.
    pub fn for_product(folder: &str) -> Result<Self, Error> {
        let base = std::env::var_os("LOCALAPPDATA").ok_or(Error::NoAppData)?;
        Self::at(PathBuf::from(base).join(folder))
    }

    /// The folder at `root`, created (with its `data` subfolder) if needed.
    pub fn at(root: PathBuf) -> Result<Self, Error> {
        fs::create_dir_all(root.join("data"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// `%LOCALAPPDATA%\DnDTools`, where DnDTools keeps its data.
    pub fn dndtools_root() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("DnDTools"))
    }

    /// Copy the market database and trained models from a DnDTools folder into ours, each only if
    /// we don't have it yet. The database is copied with SQLite's backup API, so the copy is
    /// consistent even while DnDTools is running. Returns the names copied.
    pub fn import_from_dndtools(&self, dndtools_root: &Path) -> Result<Vec<&'static str>, Error> {
        let source = dndtools_root.join("data");
        let mut copied = Vec::new();
        for name in IMPORTED {
            let (from, to) = (source.join(name), self.data().join(name));
            if !from.is_file() || to.exists() {
                continue;
            }
            // Copy under a temporary name first, so an interrupted copy is never taken for a finished one.
            let partial = to.with_file_name(format!("{name}.partial"));
            if name == MARKET_DB {
                let db = rusqlite::Connection::open_with_flags(&from, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
                db.backup(rusqlite::MAIN_DB, &partial, None)?;
            } else {
                fs::copy(&from, &partial)?;
            }
            fs::rename(&partial, &to)?;
            copied.push(name);
        }
        Ok(copied)
    }

    /// Install the starter market data that ships with the app (the database gzipped), each file
    /// only if we don't have one yet: a fresh install then has prices before its first market visit.
    /// Returns the names installed.
    pub fn install_starter(&self, starter: &Path) -> Result<Vec<&'static str>, Error> {
        let mut installed = Vec::new();
        for name in IMPORTED {
            let to = self.data().join(name);
            let from = if name == MARKET_DB { starter.join(format!("{name}.gz")) } else { starter.join(name) };
            if !from.is_file() || to.exists() {
                continue;
            }
            let partial = to.with_file_name(format!("{name}.partial"));
            if name == MARKET_DB {
                let mut gz = flate2::read::GzDecoder::new(fs::File::open(&from)?);
                std::io::copy(&mut gz, &mut fs::File::create(&partial)?)?;
            } else {
                fs::copy(&from, &partial)?;
            }
            fs::rename(&partial, &to)?;
            installed.push(name);
        }
        Ok(installed)
    }
}

/// The settings file: a JSON object, written atomically on every change.
#[derive(Debug)]
pub struct Settings {
    path: PathBuf,
    values: Map<String, Value>,
}

impl Settings {
    /// Settings at `path`: empty when the file doesn't exist; a damaged file is renamed to
    /// `<name>.damaged` (kept for recovery) and the app starts from empty settings.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let values = match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Map<String, Value>>(&text) {
                Ok(values) => values,
                Err(_) => {
                    let aside = path.with_file_name(format!(
                        "{}.damaged",
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("settings.json")
                    ));
                    fs::rename(path, aside)?;
                    Map::new()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { path: path.to_path_buf(), values })
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.values.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn set<T: Serialize>(&mut self, key: &str, value: T) -> Result<(), Error> {
        self.values.insert(key.to_owned(), serde_json::to_value(value)?);
        self.save()
    }

    pub fn all(&self) -> &Map<String, Value> {
        &self.values
    }

    fn save(&self) -> Result<(), Error> {
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&self.values)?)?;
        fs::rename(tmp, &self.path)?;
        Ok(())
    }
}
