//! Per-character quest progress: which objectives have been submitted to, and how much, plus the
//! set of merchants currently active in-game. Ported from the persistence half of
//! `quest_service.py` (`_sanitize_progress_payload`, `load_progress*`, `save_progress`,
//! `update_progress`, `save_active_merchants`, `clear_progress_file`).
//!
//! The app owns *which* character's data this reads and writes: it passes the folder (typically a
//! per-character data directory) to [`ProgressStore::new`], this crate never chooses it itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fs_util::atomic_write_json;
use crate::Error;

/// The progress file's name inside the data directory the app passes in.
pub(crate) const PROGRESS_FILE_NAME: &str = "quests_progress.json";

/// Progress toward one quest objective, keyed by an app-chosen string (Python used
/// `"{quest_id}::{type}::{index}::{item_id}"` for browser sync and `"captured::{quest}::{idx}::{id}"`
/// for packet-derived progress; this crate treats the key as opaque).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectiveProgress {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quest_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    pub submitted: u32,
    pub completed: bool,
}

/// A character's whole quest-progress state: per-objective submission counts, plus a separate item
/// counter map the UI keeps for items not tied to a single objective key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestProgress {
    #[serde(default)]
    pub objectives: HashMap<String, ObjectiveProgress>,
    #[serde(default)]
    pub items: HashMap<String, u32>,
}

/// Tolerantly rebuild a [`QuestProgress`] from whatever JSON is on disk, the way
/// `_sanitize_progress_payload` does: entries with the wrong shape (not an object, a non-numeric
/// count, ...) are dropped or defaulted instead of failing the whole load. This crate's own writes
/// always produce well-formed data; this only matters for a hand-edited or older-format file.
fn sanitize_progress_value(value: &Value) -> QuestProgress {
    let mut sanitized = QuestProgress::default();
    let Some(root) = value.as_object() else { return sanitized };

    if let Some(objectives) = root.get("objectives").and_then(Value::as_object) {
        for (key, entry) in objectives {
            if let Some(entry) = entry.as_object() {
                sanitized.objectives.insert(key.clone(), sanitize_objective_entry(entry));
            }
        }
    }

    if let Some(items) = root.get("items").and_then(Value::as_object) {
        for (key, value) in items {
            if let Some(count) = value.as_i64() {
                sanitized.items.insert(key.clone(), count.max(0) as u32);
            }
        }
    }

    sanitized
}

fn sanitize_objective_entry(entry: &serde_json::Map<String, Value>) -> ObjectiveProgress {
    let non_empty_string = |key: &str| {
        entry.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_string)
    };
    ObjectiveProgress {
        quest_id: non_empty_string("quest_id"),
        objective_index: entry.get("objective_index").and_then(Value::as_i64),
        kind: non_empty_string("type"),
        item_id: non_empty_string("item_id"),
        submitted: entry.get("submitted").and_then(Value::as_i64).map(|v| v.max(0) as u32).unwrap_or(0),
        completed: entry.get("completed").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// `int(value)` clamped to `>= 0`, the way `_coerce_progress_revision` does; a negative or
/// non-numeric revision is treated as "no revision" rather than rejected outright.
fn coerce_revision(value: &Value) -> Option<i64> {
    let parsed = value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))?;
    (parsed >= 0).then_some(parsed)
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

/// Reads and atomically writes one character's progress file. All methods serialize through an
/// internal lock, matching the Python service's single `_progress_lock` shared by every operation
/// that touches the file — so, for example, a concurrent `save_progress` and `save_active_merchants`
/// can't interleave their read-modify-write and drop one of the two updates.
pub struct ProgressStore {
    progress_file: PathBuf,
    lock: Mutex<()>,
}

impl ProgressStore {
    /// A store for the progress file inside `data_dir` (created on first write if missing).
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { progress_file: data_dir.into().join(PROGRESS_FILE_NAME), lock: Mutex::new(()) }
    }

    pub fn progress_file(&self) -> &Path {
        &self.progress_file
    }

    /// A fresh, empty progress payload. Mirrors `QuestService.default_progress_payload`.
    pub fn default_progress_payload(&self) -> QuestProgress {
        QuestProgress::default()
    }

    /// `progress`, `timestamp`, `revision` and `active_merchants` from one consistent snapshot of
    /// the file (never a half-updated mix of an old and a new write). Mirrors
    /// `load_progress_sync_state`.
    pub fn load_progress_sync_state(&self) -> (QuestProgress, Option<f64>, Option<i64>, Vec<String>) {
        let _guard = self.lock();
        let Some(payload) = self.read_raw() else {
            return (QuestProgress::default(), None, None, Vec::new());
        };

        let timestamp = payload.get("timestamp").and_then(Value::as_f64);
        let progress = payload.get("progress").map(sanitize_progress_value).unwrap_or_default();
        let revision = payload.get("progress_revision").and_then(coerce_revision);
        let active_merchants = payload
            .get("active_merchants")
            .and_then(Value::as_array)
            .map(|entries| dedupe_trimmed_strings(entries))
            .unwrap_or_default();

        (progress, timestamp, revision, active_merchants)
    }

    /// Just `progress` and `timestamp`. Mirrors `load_progress`.
    pub fn load_progress(&self) -> (QuestProgress, Option<f64>) {
        let (progress, timestamp, _revision, _active_merchants) = self.load_progress_sync_state();
        (progress, timestamp)
    }

    /// Persist `progress`. When `revision` is given and the file already holds a revision at or
    /// past it, the write is rejected (`Ok(false)`) rather than overwriting newer state — this is
    /// what lets a stale browser-tab write lose to a newer packet-derived one. With no `revision`,
    /// one is minted from the current time (monotonic with whatever revision is already on disk).
    /// `active_merchants` replaces the stored list when given, and is left untouched when `None`.
    /// Mirrors `save_progress`.
    pub fn save_progress(
        &self,
        progress: &QuestProgress,
        active_merchants: Option<&[String]>,
        revision: Option<i64>,
    ) -> Result<bool, Error> {
        let _guard = self.lock();
        let existing = self.read_raw();
        let previous_revision = existing.as_ref().and_then(|p| p.get("progress_revision")).and_then(coerce_revision);

        if let (Some(incoming), Some(previous)) = (revision, previous_revision) {
            if incoming <= previous {
                return Ok(false);
            }
        }
        let resolved_revision = revision.unwrap_or_else(|| now_millis().max(previous_revision.unwrap_or(0) + 1));

        let mut payload = serde_json::json!({
            "version": 2,
            "timestamp": now_seconds(),
            "progress_revision": resolved_revision,
            "progress": serde_json::to_value(progress)?,
        });
        self.carry_or_replace_active_merchants(&mut payload, existing.as_ref(), active_merchants);

        self.write_raw(&payload)?;
        Ok(true)
    }

    /// Atomically read-modify-write progress from a background source (the packet handler): loads
    /// the current progress, applies `updater`, and persists the result under a freshly minted
    /// revision so it always wins over whatever was there. Mirrors `update_progress`.
    pub fn update_progress(&self, updater: impl FnOnce(QuestProgress) -> QuestProgress) -> Result<bool, Error> {
        let _guard = self.lock();
        let existing = self.read_raw();
        let current = existing.as_ref().and_then(|p| p.get("progress")).map(sanitize_progress_value).unwrap_or_default();
        let updated = updater(current);

        let previous_revision = existing.as_ref().and_then(|p| p.get("progress_revision")).and_then(coerce_revision);
        let revision = now_millis().max(previous_revision.unwrap_or(0) + 1);

        let mut payload = serde_json::json!({
            "version": 2,
            "timestamp": now_seconds(),
            "progress_revision": revision,
            "progress": serde_json::to_value(&updated)?,
        });
        self.carry_or_replace_active_merchants(&mut payload, existing.as_ref(), None);

        self.write_raw(&payload)?;
        Ok(true)
    }

    /// Persist the in-game-active merchant id list, independent of quest progress. Mirrors
    /// `save_active_merchants`.
    pub fn save_active_merchants(&self, merchant_ids: &[String]) -> Result<(), Error> {
        let _guard = self.lock();
        let mut existing = self.read_raw().filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}));
        existing["active_merchants"] = serde_json::json!(merchant_ids);
        existing["timestamp"] = serde_json::json!(now_seconds());
        if existing.get("version").is_none() {
            existing["version"] = serde_json::json!(2);
        }
        self.write_raw(&existing)
    }

    /// The raw stored active-merchant id list (not deduplicated/trimmed — unlike
    /// [`Self::load_progress_sync_state`], matching `load_active_merchants`).
    pub fn load_active_merchants(&self) -> Vec<String> {
        let _guard = self.lock();
        self.read_raw()
            .and_then(|payload| payload.get("active_merchants").and_then(Value::as_array).cloned())
            .map(|entries| entries.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    /// Delete the progress file. `Ok(false)` when it didn't exist; mirrors `clear_progress_file`.
    pub fn clear_progress_file(&self) -> Result<bool, Error> {
        let _guard = self.lock();
        match std::fs::remove_file(&self.progress_file) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    fn carry_or_replace_active_merchants(
        &self,
        payload: &mut Value,
        existing: Option<&Value>,
        active_merchants: Option<&[String]>,
    ) {
        if let Some(merchants) = active_merchants {
            payload["active_merchants"] = serde_json::json!(merchants);
        } else if let Some(previous) = existing.and_then(|p| p.get("active_merchants")).filter(|v| v.is_array()) {
            payload["active_merchants"] = previous.clone();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        // A poisoned lock only means some earlier call panicked mid-write; the temp-file-then-rename
        // pattern in `write_raw` never leaves the real file half-written, so recovering is safe.
        self.lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn read_raw(&self) -> Option<Value> {
        let text = std::fs::read_to_string(&self.progress_file).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Write `payload` over the progress file atomically. Mirrors `_write_progress_file_unlocked`.
    fn write_raw(&self, payload: &Value) -> Result<(), Error> {
        atomic_write_json(&self.progress_file, payload)
    }
}

/// Trim, drop-empty, and deduplicate (keeping first occurrence) a JSON array of strings.
fn dedupe_trimmed_strings(entries: &[Value]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for entry in entries {
        let Some(text) = entry.as_str() else { continue };
        let trimmed = text.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            result.push(trimmed.to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn store_in_temp_dir() -> (tempfile::TempDir, ProgressStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ProgressStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn progress_and_active_merchants_are_transactional_under_concurrent_saves() {
        let (dir, store) = store_in_temp_dir();
        let store = Arc::new(store);

        for revision in 1..=40i64 {
            let mut progress = QuestProgress::default();
            progress.objectives.insert(
                "quest:objective".to_string(),
                ObjectiveProgress { submitted: revision as u32, ..Default::default() },
            );
            let merchants = vec![format!("merchant-{revision}")];

            let progress_thread = {
                let store = store.clone();
                let progress = progress.clone();
                std::thread::spawn(move || store.save_progress(&progress, None, Some(revision)))
            };
            let merchants_thread = {
                let store = store.clone();
                let merchants = merchants.clone();
                std::thread::spawn(move || store.save_active_merchants(&merchants))
            };

            assert!(progress_thread.join().expect("thread panicked").expect("save_progress failed"));
            merchants_thread.join().expect("thread panicked").expect("save_active_merchants failed");

            let (loaded_progress, _timestamp) = store.load_progress();
            assert_eq!(loaded_progress.objectives["quest:objective"].submitted, revision as u32);
            assert_eq!(store.load_active_merchants(), merchants);
        }

        let leftover_temp_files: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read temp dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover_temp_files.is_empty(), "leftover temp files: {leftover_temp_files:?}");
    }

    #[test]
    fn older_progress_revision_cannot_overwrite_newer_state() {
        let (_dir, store) = store_in_temp_dir();
        let mut newer = QuestProgress::default();
        newer.items.insert("Bandage".to_string(), 9);
        let mut older = QuestProgress::default();
        older.items.insert("Bandage".to_string(), 2);

        assert!(store.save_progress(&newer, Some(&["Alchemist".to_string()]), Some(200)).expect("save"));
        assert!(!store.save_progress(&older, Some(&["Armourer".to_string()]), Some(100)).expect("save"));

        let (progress, _timestamp) = store.load_progress();
        assert_eq!(progress.items["Bandage"], 9);
        assert_eq!(store.load_active_merchants(), vec!["Alchemist".to_string()]);
    }

    #[test]
    fn progress_sync_snapshot_includes_revision_and_merchants() {
        let (_dir, store) = store_in_temp_dir();
        let mut progress = QuestProgress::default();
        progress.items.insert("Bandage".to_string(), 4);
        let merchants = vec!["Alchemist".to_string(), "Alchemist".to_string(), "  Armourer  ".to_string()];

        assert!(store.save_progress(&progress, Some(&merchants), Some(321)).expect("save"));

        let (loaded, timestamp, revision, active_merchants) = store.load_progress_sync_state();
        assert_eq!(loaded.items["Bandage"], 4);
        assert!(timestamp.is_some());
        assert_eq!(revision, Some(321));
        assert_eq!(active_merchants, vec!["Alchemist".to_string(), "Armourer".to_string()]);
    }

    #[test]
    fn clear_progress_file_reports_whether_a_file_existed() {
        let (_dir, store) = store_in_temp_dir();
        assert!(!store.clear_progress_file().expect("clear"));

        store.save_progress(&QuestProgress::default(), None, None).expect("save");
        assert!(store.clear_progress_file().expect("clear"));
    }

    #[test]
    fn sanitize_drops_malformed_entries_and_clamps_negative_counts() {
        let value = serde_json::json!({
            "objectives": {
                "ok": {"submitted": -3, "completed": true},
                "bad": "not an object",
            },
            "items": {"Bandage": -1, "Rope": 4},
        });

        let sanitized = sanitize_progress_value(&value);

        assert_eq!(sanitized.objectives.len(), 1);
        assert_eq!(sanitized.objectives["ok"].submitted, 0);
        assert!(sanitized.objectives["ok"].completed);
        assert_eq!(sanitized.items["Bandage"], 0);
        assert_eq!(sanitized.items["Rope"], 4);
    }
}
