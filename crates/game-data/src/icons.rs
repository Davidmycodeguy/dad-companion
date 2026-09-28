//! The icon pack: every item icon (WebP) in one LZMA-compressed zip, `assets/icons.pak`.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use zip::ZipArchive;

use crate::Error;

/// Icons read on demand from the pack, remembered once read.
pub struct IconPack {
    archive: Mutex<ZipArchive<File>>,
    names: std::collections::HashSet<String>,
    cache: Mutex<HashMap<String, std::sync::Arc<Vec<u8>>>>,
}

impl IconPack {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let archive = ZipArchive::new(File::open(path)?)?;
        let names = archive.file_names().map(str::to_owned).collect();
        Ok(Self { archive: Mutex::new(archive), names, cache: Mutex::new(HashMap::new()) })
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The icon's bytes for any spelling of its path (see [`canonical_icon_path`]), or None.
    pub fn get(&self, path: &str) -> Option<std::sync::Arc<Vec<u8>>> {
        let key = canonical_icon_path(path)?;
        if let Some(hit) = self.cache.lock().ok()?.get(&key) {
            return Some(hit.clone());
        }
        if !self.names.contains(&key) {
            return None;
        }
        let mut bytes = Vec::new();
        {
            let mut archive = self.archive.lock().ok()?;
            archive.by_name(&key).ok()?.read_to_end(&mut bytes).ok()?;
        }
        let bytes = std::sync::Arc::new(bytes);
        self.cache.lock().ok()?.insert(key, bytes.clone());
        Some(bytes)
    }
}

/// "icons/<folder>/<name>.webp" for an icon path however it was written: backslashes, a leading
/// "assets/" or "./", a missing "icons/" prefix or another image extension.
pub fn canonical_icon_path(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.first().is_some_and(|p| p.eq_ignore_ascii_case("assets")) {
        parts.remove(0);
    }
    if parts.is_empty() {
        return None;
    }
    if !parts[0].eq_ignore_ascii_case("icons") {
        parts.insert(0, "icons");
    }
    let file = parts.pop()?;
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    Some(format!("{}/{stem}.webp", parts.join("/")))
}
