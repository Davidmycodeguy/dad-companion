//! Port of `sort_event_store.py`'s `SortEventStore`: a local, queryable log of sort-related events
//! used to train the adaptive model. Five event kinds, matching Python's five —
//! `MoveOutcome` is carried over unused by any trainer, same as in Python; see [`super::model`]'s
//! doc comment for why that asymmetry is intentional, not a gap:
//!   - `SortStarted`   — features + plan snapshot at sort begin
//!   - `SortCompleted` — outcome (success/fail) + metrics at sort end
//!   - `ItemPlacement` — per-item placement record (candidate features + outcome)
//!   - `UserCorrection`— detected manual move after sort (before/after positions)
//!   - `MoveOutcome`   — outcome of one physical move (reserved for future move-failure prediction)
//!
//! ## Difference from the Python: storage details
//!
//! Same engine (SQLite; the task brief allows either SQLite or JSON Lines) and close to the same
//! schema. `rusqlite`'s `bundled` feature compiles SQLite from source, so there is no system SQLite
//! dependency — `crates/appdata` and `crates/market` already depend on it the same way elsewhere in
//! this workspace. The one structural difference: Python keeps one SQLite connection per thread
//! (`threading.local()`) so concurrent readers do not block each other under WAL mode. This port
//! uses a single connection behind a `Mutex`, which is simpler and plenty fast for a local,
//! write-mostly event log at this app's scale — there is no multi-reader hot path to protect here.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension, ToSql};
use serde_json::Value;

use super::model::{CORRECTION_WEIGHT, FAILURE_WEIGHT};
use super::{now_secs, EventSink, TrainingSample, UserCorrection};

/// Auto-prune once the event count crosses a multiple of this many inserts, matching
/// `sort_event_store.py`'s `row_id % 1000 == 0` check.
const PRUNE_CHECK_INTERVAL: i64 = 1000;
/// Default cap passed to that periodic prune, matching Python's `MAX_EVENTS`.
const MAX_EVENTS: i64 = 50_000;
/// A user-supplied feedback note is stored at most this many characters, matching Python's
/// `str(note)[:500]`.
const MAX_NOTE_CHARS: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not create the event store's folder {path}: {source}")]
    CreateDir { path: PathBuf, source: std::io::Error },
    #[error("could not open the event store at {path}: {source}")]
    Open { path: PathBuf, source: rusqlite::Error },
    #[error("event store operation failed: {0}")]
    Sql(#[from] rusqlite::Error),
}

/// The kind of thing an event records. Stored in SQLite as `as_str()`, the same literal strings
/// (`"SORT_STARTED"`, ...) Python stores, so an existing database written by either side reads
/// identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    SortStarted,
    SortCompleted,
    ItemPlacement,
    UserCorrection,
    MoveOutcome,
}

impl EventKind {
    fn as_str(self) -> &'static str {
        match self {
            EventKind::SortStarted => "SORT_STARTED",
            EventKind::SortCompleted => "SORT_COMPLETED",
            EventKind::ItemPlacement => "ITEM_PLACEMENT",
            EventKind::UserCorrection => "USER_CORRECTION",
            EventKind::MoveOutcome => "MOVE_OUTCOME",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        Some(match value {
            "SORT_STARTED" => EventKind::SortStarted,
            "SORT_COMPLETED" => EventKind::SortCompleted,
            "ITEM_PLACEMENT" => EventKind::ItemPlacement,
            "USER_CORRECTION" => EventKind::UserCorrection,
            "MOVE_OUTCOME" => EventKind::MoveOutcome,
            _ => return None,
        })
    }
}

/// A full stored event row, as returned by [`SortEventStore::get_unsynced_events`]. Mirrors
/// Python's `_row_to_full_dict`.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub id: i64,
    pub kind: EventKind,
    pub session_id: String,
    pub timestamp: f64,
    pub features: HashMap<String, f64>,
    pub label: Option<bool>,
    pub weight: f64,
    pub metadata: Value,
}

/// Thread-safe, SQLite-backed event store for sort ML events.
pub struct SortEventStore {
    conn: Mutex<Connection>,
}

impl SortEventStore {
    /// Opens (creating if needed) the event database under `base_dir`. Matches
    /// `sort_event_store.py`'s `SortEventStore(base_dir)`, except `base_dir` is required: the
    /// Python default of an app-wide output directory belongs to the app that embeds this crate.
    pub fn new(base_dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        let base_dir = base_dir.as_ref();
        fs::create_dir_all(base_dir)
            .map_err(|source| StoreError::CreateDir { path: base_dir.to_path_buf(), source })?;

        let db_path = base_dir.join("events.db");
        let conn = Connection::open(&db_path).map_err(|source| StoreError::Open { path: db_path, source })?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             CREATE TABLE IF NOT EXISTS events (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_type  TEXT    NOT NULL,
                 session_id  TEXT    NOT NULL,
                 timestamp   REAL    NOT NULL,
                 features    TEXT,
                 label       INTEGER,
                 weight      REAL    DEFAULT 1.0,
                 metadata    TEXT,
                 synced      INTEGER DEFAULT 0,
                 created_at  REAL    NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_events_session   ON events (session_id);
             CREATE INDEX IF NOT EXISTS idx_events_type      ON events (event_type);
             CREATE INDEX IF NOT EXISTS idx_events_synced    ON events (synced);
             CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events (timestamp);",
        )?;

        Ok(SortEventStore { conn: Mutex::new(conn) })
    }

    fn insert_event(
        &self,
        kind: EventKind,
        session_id: &str,
        features: Option<&HashMap<String, f64>>,
        label: Option<bool>,
        weight: f64,
        metadata: Option<&Value>,
    ) -> Result<i64, StoreError> {
        let now = now_secs();
        let features_json = features.and_then(|f| serde_json::to_string(f).ok());
        let metadata_json = metadata.map(ToString::to_string);

        let row_id = {
            let conn = super::lock(&self.conn);
            conn.execute(
                "INSERT INTO events
                     (event_type, session_id, timestamp, features, label, weight, metadata, synced, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
                params![kind.as_str(), session_id, now, features_json, label.map(|b| b as i64), weight, metadata_json, now],
            )?;
            conn.last_insert_rowid()
        };

        // Periodic prune, mirroring sort_event_store.py's `_insert_event`. The lock above is
        // already released by this point, since `prune` takes its own lock.
        if row_id % PRUNE_CHECK_INTERVAL == 0 {
            let _ = self.prune(MAX_EVENTS);
        }
        Ok(row_id)
    }

    pub fn record_sort_started(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        metadata: Value,
    ) -> Result<i64, StoreError> {
        self.insert_event(EventKind::SortStarted, session_id, Some(features), None, 1.0, Some(&metadata))
    }

    /// `label` is `true` (1) for a failed session and `false` (0) for a success — "1 = failure for
    /// the risk model", as the Python comment puts it — and a failure carries `FAILURE_WEIGHT`.
    pub fn record_sort_completed(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        success: bool,
        mut metadata: Value,
    ) -> Result<i64, StoreError> {
        if let Value::Object(map) = &mut metadata {
            map.insert("success".to_string(), Value::Bool(success));
        }
        let weight = if success { 1.0 } else { FAILURE_WEIGHT };
        self.insert_event(EventKind::SortCompleted, session_id, Some(features), Some(!success), weight, Some(&metadata))
    }

    /// `label` is `true` (1) for a good placement and `false` (0) for a bad one.
    pub fn record_item_placement(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        success: bool,
        metadata: Value,
    ) -> Result<i64, StoreError> {
        self.insert_event(EventKind::ItemPlacement, session_id, Some(features), Some(success), 1.0, Some(&metadata))
    }

    /// Records two events for one correction: a negative sample at the planned position and a
    /// positive sample at the user's chosen position, both weighted by `CORRECTION_WEIGHT`.
    pub fn record_user_correction(&self, session_id: &str, correction: UserCorrection) -> Result<Vec<i64>, StoreError> {
        let UserCorrection { item_id, planned_features, corrected_features, planned_pos, corrected_pos } = correction;

        let planned_meta = serde_json::json!({
            "item_id": item_id,
            "correction_role": "planned_failure",
            "position": {"x": planned_pos.0, "y": planned_pos.1},
        });
        let corrected_meta = serde_json::json!({
            "item_id": item_id,
            "correction_role": "manual_success",
            "position": {"x": corrected_pos.0, "y": corrected_pos.1},
        });

        let negative_id = self.insert_event(
            EventKind::UserCorrection,
            session_id,
            Some(&planned_features),
            Some(false),
            CORRECTION_WEIGHT,
            Some(&planned_meta),
        )?;
        let positive_id = self.insert_event(
            EventKind::UserCorrection,
            session_id,
            Some(&corrected_features),
            Some(true),
            CORRECTION_WEIGHT,
            Some(&corrected_meta),
        )?;
        Ok(vec![negative_id, positive_id])
    }

    /// Records the outcome of one physical move. Reserved for a future move-failure predictor; see
    /// the module doc comment.
    pub fn record_move_outcome(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        verified: bool,
        attempts: u32,
        item_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<i64, StoreError> {
        let mut metadata = serde_json::json!({ "attempts": attempts });
        if let (Value::Object(map), Some(id)) = (&mut metadata, item_id) {
            map.insert("item_id".to_string(), Value::String(id.to_string()));
        }
        if let (Value::Object(map), Some(r)) = (&mut metadata, reason) {
            map.insert("reason".to_string(), Value::String(r.to_string()));
        }
        let weight = if verified { 1.0 } else { FAILURE_WEIGHT };
        self.insert_event(EventKind::MoveOutcome, session_id, Some(features), Some(verified), weight, Some(&metadata))
    }

    /// `SORT_COMPLETED` events formatted for risk-model training.
    pub fn get_risk_training_data(&self, limit: i64) -> Result<Vec<TrainingSample>, StoreError> {
        self.query_training_events(EventKind::SortCompleted, limit)
    }

    /// `ITEM_PLACEMENT` and `USER_CORRECTION` events formatted for item-model training.
    pub fn get_item_training_data(&self, limit: i64) -> Result<Vec<TrainingSample>, StoreError> {
        let conn = super::lock(&self.conn);
        let mut statement = conn.prepare(
            "SELECT features, label, weight FROM events
             WHERE event_type IN (?1, ?2) AND label IS NOT NULL
             ORDER BY timestamp DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(
            params![EventKind::ItemPlacement.as_str(), EventKind::UserCorrection.as_str(), limit],
            row_to_training_sample,
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn query_training_events(&self, kind: EventKind, limit: i64) -> Result<Vec<TrainingSample>, StoreError> {
        let conn = super::lock(&self.conn);
        let mut statement = conn.prepare(
            "SELECT features, label, weight FROM events
             WHERE event_type = ?1 AND label IS NOT NULL
             ORDER BY timestamp DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![kind.as_str(), limit], row_to_training_sample)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Events not yet marked synced, oldest first. The name and "synced" bookkeeping are kept from
    /// Python's server-sync design for parity, even though nothing in this app ever syncs anywhere
    /// any more (see the crate-level doc comment); `apply_user_feedback` still uses "unsynced" to
    /// mean "needs a retrain to pick this up".
    pub fn get_unsynced_events(&self, limit: i64) -> Result<Vec<StoredEvent>, StoreError> {
        let conn = super::lock(&self.conn);
        let mut statement = conn.prepare(
            "SELECT id, event_type, session_id, timestamp, features, label, weight, metadata
             FROM events WHERE synced = 0 ORDER BY timestamp ASC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], row_to_stored_event)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn mark_synced(&self, event_ids: &[i64]) -> Result<(), StoreError> {
        if event_ids.is_empty() {
            return Ok(());
        }
        let placeholders = vec!["?"; event_ids.len()].join(",");
        let sql = format!("UPDATE events SET synced = 1 WHERE id IN ({placeholders})");
        let ids: Vec<&dyn ToSql> = event_ids.iter().map(|id| id as &dyn ToSql).collect();

        let conn = super::lock(&self.conn);
        conn.execute(&sql, ids.as_slice())?;
        Ok(())
    }

    /// Overrides the most recent `SORT_COMPLETED` sample for `session_id` with explicit user
    /// feedback, in place, and re-queues it (`synced = 0`).
    ///
    /// Manual feedback belongs to the actual completed session's own feature vector, so this
    /// updates that row instead of inserting a second, feature-less training record — the same
    /// reasoning `sort_event_store.py`'s `apply_user_feedback` docstring gives. Returns `false`
    /// when no `SORT_COMPLETED` event exists yet for `session_id`.
    pub fn apply_user_feedback(&self, session_id: &str, success: bool, note: Option<&str>) -> Result<bool, StoreError> {
        let conn = super::lock(&self.conn);

        let existing: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT id, metadata FROM events
                 WHERE event_type = ?1 AND session_id = ?2
                 ORDER BY timestamp DESC, id DESC LIMIT 1",
                params![EventKind::SortCompleted.as_str(), session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((id, metadata_json)) = existing else { return Ok(false) };

        let mut metadata: Value =
            metadata_json.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_else(|| Value::Object(Default::default()));
        if !metadata.is_object() {
            metadata = Value::Object(Default::default());
        }
        // Just ensured `metadata` is `Value::Object(..)` above, so this always succeeds.
        let fields = metadata.as_object_mut().expect("metadata was just normalized to an object");
        fields.insert("user_feedback".to_string(), Value::Bool(true));
        fields.insert("user_success".to_string(), Value::Bool(success));
        match note {
            Some(text) => {
                fields.insert("user_note".to_string(), Value::String(text.chars().take(MAX_NOTE_CHARS).collect()));
            }
            None => {
                fields.remove("user_note");
            }
        }

        let label = !success;
        let weight = if success { 1.0 } else { FAILURE_WEIGHT };
        let changed = conn.execute(
            "UPDATE events SET label = ?1, weight = ?2, metadata = ?3, synced = 0 WHERE id = ?4",
            params![label as i64, weight, metadata.to_string(), id],
        )?;
        Ok(changed == 1)
    }

    pub fn count_events(&self) -> Result<i64, StoreError> {
        let conn = super::lock(&self.conn);
        Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?)
    }

    /// The most recent timestamp among synced events, if any.
    pub fn last_sync_cursor(&self) -> Result<Option<f64>, StoreError> {
        let conn = super::lock(&self.conn);
        Ok(conn.query_row("SELECT MAX(timestamp) FROM events WHERE synced = 1", [], |row| row.get(0))?)
    }

    pub fn session_ids(&self, limit: i64) -> Result<Vec<String>, StoreError> {
        let conn = super::lock(&self.conn);
        let mut statement = conn.prepare("SELECT DISTINCT session_id FROM events ORDER BY timestamp DESC LIMIT ?1")?;
        let rows = statement.query_map(params![limit], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Deletes the oldest *synced* events beyond `keep`, so the database does not grow without
    /// bound. Matches `sort_event_store.py`'s `prune`; called automatically every
    /// `PRUNE_CHECK_INTERVAL` inserts, and safe to call any time otherwise.
    pub fn prune(&self, keep: i64) -> Result<i64, StoreError> {
        let conn = super::lock(&self.conn);
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        if total <= keep {
            return Ok(0);
        }
        let excess = total - keep;
        let deleted = conn.execute(
            "DELETE FROM events WHERE id IN (
                 SELECT id FROM events WHERE synced = 1 ORDER BY timestamp ASC LIMIT ?1
             )",
            params![excess],
        )?;
        Ok(deleted as i64)
    }

    /// Closes the real on-disk connection now rather than whenever `self` eventually drops, then
    /// replaces it with a throwaway in-memory one so `self` stays valid to hold (matching the
    /// "open, use, close" lifecycle Python's tests exercise; this store has no further use once
    /// closed, so callers should drop it soon after).
    pub fn close(&self) {
        let mut conn = super::lock(&self.conn);
        if let Ok(memory) = Connection::open_in_memory() {
            *conn = memory;
        }
    }
}

fn row_to_training_sample(row: &rusqlite::Row) -> rusqlite::Result<TrainingSample> {
    let features_json: Option<String> = row.get(0)?;
    let label: i64 = row.get(1)?;
    let weight: f64 = row.get(2)?;
    let features = features_json.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    Ok(TrainingSample { features, label: label != 0, weight })
}

fn row_to_stored_event(row: &rusqlite::Row) -> rusqlite::Result<StoredEvent> {
    let id: i64 = row.get(0)?;
    let kind_text: String = row.get(1)?;
    let session_id: String = row.get(2)?;
    let timestamp: f64 = row.get(3)?;
    let features_json: Option<String> = row.get(4)?;
    let label: Option<i64> = row.get(5)?;
    let weight: f64 = row.get(6)?;
    let metadata_json: Option<String> = row.get(7)?;

    Ok(StoredEvent {
        id,
        // Every row this store ever writes uses a name from `EventKind::as_str`; falling back to
        // `SortStarted` here only matters for a hand-edited or foreign-written database.
        kind: EventKind::from_str(&kind_text).unwrap_or(EventKind::SortStarted),
        session_id,
        timestamp,
        features: features_json.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default(),
        label: label.map(|value| value != 0),
        weight,
        metadata: metadata_json.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null),
    })
}

impl EventSink for SortEventStore {
    fn record_sort_started(&self, session_id: &str, features: &HashMap<String, f64>, metadata: Value) -> Result<i64, StoreError> {
        SortEventStore::record_sort_started(self, session_id, features, metadata)
    }

    fn record_sort_completed(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        success: bool,
        metadata: Value,
    ) -> Result<i64, StoreError> {
        SortEventStore::record_sort_completed(self, session_id, features, success, metadata)
    }

    fn record_item_placement(
        &self,
        session_id: &str,
        features: &HashMap<String, f64>,
        success: bool,
        metadata: Value,
    ) -> Result<i64, StoreError> {
        SortEventStore::record_item_placement(self, session_id, features, success, metadata)
    }

    fn record_user_correction(&self, session_id: &str, correction: UserCorrection) -> Result<Vec<i64>, StoreError> {
        SortEventStore::record_user_correction(self, session_id, correction)
    }

    fn apply_user_feedback(&self, session_id: &str, success: bool, note: Option<&str>) -> Result<bool, StoreError> {
        SortEventStore::apply_user_feedback(self, session_id, success, note)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, SortEventStore) {
        let dir = tempfile::tempdir().expect("create temp dir for test store");
        let store = SortEventStore::new(dir.path()).expect("open event store");
        (dir, store)
    }

    fn features(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn a_completed_session_round_trips_through_risk_training_data() {
        // Arrange
        let (_dir, store) = temp_store();

        // Act
        store
            .record_sort_completed("s1", &features(&[("stash_fill_ratio", 0.5)]), false, Value::Object(Default::default()))
            .expect("record sort completed");
        let rows = store.get_risk_training_data(100).expect("query risk training data");

        // Assert: a failure is label=true (1) and carries the failure weight.
        assert_eq!(rows.len(), 1);
        assert!(rows[0].label);
        assert_eq!(rows[0].weight, FAILURE_WEIGHT);
        assert_eq!(rows[0].features.get("stash_fill_ratio"), Some(&0.5));
    }

    #[test]
    fn a_user_correction_writes_a_negative_and_a_positive_sample() {
        // Arrange
        let (_dir, store) = temp_store();
        let correction = UserCorrection {
            item_id: "potion".to_string(),
            planned_features: features(&[("slot_x", 1.0)]),
            corrected_features: features(&[("slot_x", 9.0)]),
            planned_pos: (1, 1),
            corrected_pos: (9, 9),
        };

        // Act
        let ids = store.record_user_correction("s1", correction).expect("record correction");
        let rows = store.get_item_training_data(100).expect("query item training data");

        // Assert
        assert_eq!(ids.len(), 2);
        assert_eq!(rows.len(), 2);
        let positives = rows.iter().filter(|r| r.label).count();
        let negatives = rows.iter().filter(|r| !r.label).count();
        assert_eq!((positives, negatives), (1, 1));
        assert!(rows.iter().all(|r| r.weight == CORRECTION_WEIGHT));
    }

    #[test]
    fn user_feedback_updates_the_existing_row_instead_of_inserting_a_new_one() {
        // Arrange
        let (_dir, store) = temp_store();
        let id = store
            .record_sort_completed("s1", &features(&[("stash_fill_ratio", 0.75)]), true, Value::Object(Default::default()))
            .expect("record sort completed");
        store.mark_synced(&[id]).expect("mark synced");

        // Act
        let accepted = store.apply_user_feedback("s1", false, Some("items got stuck")).expect("apply feedback");

        // Assert
        assert!(accepted);
        assert_eq!(store.count_events().expect("count"), 1, "feedback must update, not insert");
        let unsynced = store.get_unsynced_events(10).expect("query unsynced");
        assert_eq!(unsynced.len(), 1);
        let event = &unsynced[0];
        assert_eq!(event.id, id);
        assert_eq!(event.features.get("stash_fill_ratio"), Some(&0.75));
        assert_eq!(event.label, Some(true));
        assert_eq!(event.metadata["user_feedback"], Value::Bool(true));
        assert_eq!(event.metadata["user_success"], Value::Bool(false));
        assert_eq!(event.metadata["user_note"], Value::String("items got stuck".to_string()));
    }

    #[test]
    fn user_feedback_for_an_unknown_session_is_rejected() {
        // Arrange
        let (_dir, store) = temp_store();

        // Act / Assert
        assert!(!store.apply_user_feedback("missing-session", true, None).expect("apply feedback"));
    }

    #[test]
    fn prune_deletes_oldest_synced_events_first_and_never_touches_unsynced_ones() {
        // Arrange: one old unsynced event, then five synced events newer than it (6 total).
        let (_dir, store) = temp_store();
        let unsynced_id = store
            .record_sort_completed("s", &features(&[]), true, Value::Object(Default::default()))
            .expect("record unsynced");
        let mut synced_ids = Vec::new();
        for i in 0..5 {
            let id = store
                .record_sort_completed("s", &features(&[("i", i as f64)]), true, Value::Object(Default::default()))
                .expect("record");
            synced_ids.push(id);
        }
        store.mark_synced(&synced_ids).expect("mark synced");

        // Act: keep only 2 events, so the 4 oldest synced ones must go.
        let deleted = store.prune(2).expect("prune");

        // Assert
        assert_eq!(deleted, 4);
        assert_eq!(store.count_events().expect("count"), 2);
        let remaining_unsynced: Vec<i64> = store.get_unsynced_events(10).expect("unsynced").iter().map(|e| e.id).collect();
        assert_eq!(remaining_unsynced, vec![unsynced_id], "the unsynced event must survive pruning even though it is oldest");
    }
}
