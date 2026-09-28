//! Persistence for the packet handler's captured quest state: an opaque JSON blob so it survives an
//! app restart. Ported from `QuestService.save_captured_state` / `load_captured_state` /
//! `clear_captured_state`.
//!
//! Unlike the progress file, Python wrote this one with a plain `open(..., "w")` (no atomic
//! rename). This store writes it the same atomic way as [`crate::progress::ProgressStore`] instead —
//! a strict improvement with no behavioral cost — rather than reproducing that one inconsistency.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::fs_util::atomic_write_json;
use crate::Error;

const CAPTURED_STATE_FILE_NAME: &str = "quests_captured.json";

/// Reads and atomically writes the captured-state file inside one character's data directory.
pub struct CapturedStateStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl CapturedStateStore {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { path: data_dir.into().join(CAPTURED_STATE_FILE_NAME), lock: Mutex::new(()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Persist `state` (whatever JSON shape the packet handler produced) with the current time.
    /// Mirrors `save_captured_state`, but propagates I/O failures instead of only logging them.
    pub fn save(&self, state: &Value) -> Result<(), Error> {
        let _guard = self.lock();
        let payload = serde_json::json!({
            "version": 1,
            "timestamp": now_seconds(),
            "state": state,
        });
        atomic_write_json(&self.path, &payload)
    }

    /// The last saved state and when it was saved, or `(None, None)` when nothing is stored (or the
    /// file is missing/unreadable). Mirrors `load_captured_state`.
    pub fn load(&self) -> (Option<Value>, Option<f64>) {
        let _guard = self.lock();
        let Ok(text) = std::fs::read_to_string(&self.path) else { return (None, None) };
        let Ok(payload) = serde_json::from_str::<Value>(&text) else { return (None, None) };
        let timestamp = payload.get("timestamp").and_then(Value::as_f64);
        let state = payload.get("state").cloned();
        (state, timestamp)
    }

    /// Remove the captured-state file. `Ok(false)` when it didn't exist. Mirrors
    /// `clear_captured_state`.
    pub fn clear(&self) -> Result<bool, Error> {
        let _guard = self.lock();
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn now_seconds() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_arbitrary_state_and_clears_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = CapturedStateStore::new(dir.path().to_path_buf());

        assert_eq!(store.load(), (None, None));

        let state = serde_json::json!({"merchant_quests": {"Alchemist": []}});
        store.save(&state).expect("save");

        let (loaded, timestamp) = store.load();
        assert_eq!(loaded, Some(state));
        assert!(timestamp.is_some());

        assert!(store.clear().expect("clear"));
        assert!(!store.clear().expect("clear again"));
        assert_eq!(store.load(), (None, None));
    }
}
