//! The facade the UI queries: the quest catalog, per-character progress, and item metadata combined
//! behind one type, the way `QuestService` is in Python. Internally this is composition, not one big
//! struct: [`QuestCatalog`], [`ProgressStore`], [`CapturedStateStore`] and [`ItemFamilyIndex`] each
//! stay independently constructible and testable (see their own modules); `QuestService` exists so
//! the app has one thing to hold per character instead of four.

use std::collections::HashSet;
use std::path::PathBuf;

use game_data::ItemCatalog;

use crate::captured_state::CapturedStateStore;
use crate::catalog::{Quest, QuestCatalog};
use crate::items::{
    build_item_payload as build_item_payload_fn, merge_item_family_holdings as merge_item_family_holdings_fn,
    CharacterHolding, IconUrlBuilder, ItemFamilyIndex, ItemInfo, ItemPayload,
};
use crate::progress::{ProgressStore, QuestProgress};
use crate::text;
use crate::Error;

/// One character's quest data: the (shared, read-only) quest catalog and item family index, plus
/// this character's own progress and captured-state files under `data_dir`.
pub struct QuestService {
    catalog: QuestCatalog,
    families: ItemFamilyIndex,
    progress: ProgressStore,
    captured_state: CapturedStateStore,
}

impl QuestService {
    /// `catalog` is typically loaded once and shared; `item_catalog` is only needed transiently, to
    /// build the item-family index (see [`ItemFamilyIndex::build`]). `data_dir` is the folder the app
    /// has chosen for this character's own writable quest data.
    pub fn new(catalog: QuestCatalog, item_catalog: &ItemCatalog, data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            catalog,
            families: ItemFamilyIndex::build(item_catalog),
            progress: ProgressStore::new(data_dir.clone()),
            captured_state: CapturedStateStore::new(data_dir),
        }
    }

    /// Filenames (lowercased) this service owns, so a "clear this character's data" sweep elsewhere
    /// in the app can skip them instead of deleting out from under it. Mirrors `protected_filenames`
    /// (minus the DarkerDB network cache file, which this crate never writes — see
    /// `crates/quests/src/catalog.rs`'s doc comment).
    pub fn protected_filenames(&self) -> HashSet<String> {
        [self.progress.progress_file(), self.captured_state.path()]
            .into_iter()
            .filter_map(|path| path.file_name())
            .filter_map(|name| name.to_str())
            .map(str::to_lowercase)
            .collect()
    }

    // ---------------------------------------------------------------- Catalog queries

    pub fn quest(&self, id: &str) -> Option<&Quest> {
        self.catalog.get(id)
    }

    pub fn catalog(&self) -> &QuestCatalog {
        &self.catalog
    }

    pub fn quests(&self) -> impl Iterator<Item = &Quest> {
        self.catalog.iter()
    }

    pub fn quests_for_merchant<'a>(&'a self, merchant: &'a str) -> impl Iterator<Item = &'a Quest> {
        self.catalog.for_merchant(merchant)
    }

    pub fn quest_count(&self) -> usize {
        self.catalog.len()
    }

    // ---------------------------------------------------------------- Item/merchant helpers

    pub fn normalize_merchant_name(&self, name: Option<&str>) -> String {
        text::normalize_merchant_name(name)
    }

    pub fn normalize_item_name(&self, item_id: &str) -> String {
        text::normalize_item_name(item_id)
    }

    /// Every concrete stash item id `item_id` (an archetype, or already-concrete id) resolves to.
    pub fn concrete_item_ids(&self, item_id: &str) -> Vec<String> {
        self.families.concrete_item_ids(item_id)
    }

    pub fn item_families(&self) -> &ItemFamilyIndex {
        &self.families
    }

    pub fn build_item_payload(&self, item_info: Option<&ItemInfo>, icon_url_builder: Option<&IconUrlBuilder<'_>>) -> ItemPayload {
        build_item_payload_fn(item_info, icon_url_builder)
    }

    pub fn merge_item_family_holdings(
        &self,
        holdings_by_item: &std::collections::HashMap<String, Vec<CharacterHolding>>,
        concrete_item_ids: &[String],
    ) -> Vec<CharacterHolding> {
        merge_item_family_holdings_fn(holdings_by_item, concrete_item_ids)
    }

    // ---------------------------------------------------------------- Progress persistence

    pub fn default_progress_payload(&self) -> QuestProgress {
        self.progress.default_progress_payload()
    }

    pub fn load_progress(&self) -> (QuestProgress, Option<f64>) {
        self.progress.load_progress()
    }

    pub fn load_progress_sync_state(&self) -> (QuestProgress, Option<f64>, Option<i64>, Vec<String>) {
        self.progress.load_progress_sync_state()
    }

    pub fn save_progress(
        &self,
        progress: &QuestProgress,
        active_merchants: Option<&[String]>,
        revision: Option<i64>,
    ) -> Result<bool, Error> {
        self.progress.save_progress(progress, active_merchants, revision)
    }

    pub fn update_progress(&self, updater: impl FnOnce(QuestProgress) -> QuestProgress) -> Result<bool, Error> {
        self.progress.update_progress(updater)
    }

    pub fn save_active_merchants(&self, merchant_ids: &[String]) -> Result<(), Error> {
        self.progress.save_active_merchants(merchant_ids)
    }

    pub fn load_active_merchants(&self) -> Vec<String> {
        self.progress.load_active_merchants()
    }

    pub fn clear_progress_file(&self) -> Result<bool, Error> {
        self.progress.clear_progress_file()
    }

    /// The underlying store, for a caller (such as [`crate::packet_handler::QuestPacketHandler`])
    /// that needs to sync progress directly rather than through this facade.
    pub fn progress_store(&self) -> &ProgressStore {
        &self.progress
    }

    // ---------------------------------------------------------------- Captured state

    pub fn save_captured_state(&self, state: &serde_json::Value) -> Result<(), Error> {
        self.captured_state.save(state)
    }

    pub fn load_captured_state(&self) -> (Option<serde_json::Value>, Option<f64>) {
        self.captured_state.load()
    }

    pub fn clear_captured_state(&self) -> Result<bool, Error> {
        self.captured_state.clear()
    }
}
