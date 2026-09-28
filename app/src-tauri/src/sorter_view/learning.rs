//! Sort learning behind the sorter: the local model and event store, trained in the background
//! after runs (at most every half hour, sooner right after new feedback), and the plan each run
//! registers so a manual move afterwards is learned from. Nothing here leaves the machine.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use sorter::learn::{ItemPosition, SortAdaptiveModel, SortEventStore, SortLearningManager, SortObserver, StoreError, TrainScheduler};
use sorter::plan::SortPlan;
use state::{grid_size, slot_cell, Character};

/// Most recent samples each training reads, per head.
const TRAINING_SAMPLES: i64 = 5_000;

pub struct Learning {
    pub model: SortAdaptiveModel,
    pub store: Arc<SortEventStore>,
    pub observer: SortObserver,
    manager: SortLearningManager,
    scheduler: Arc<Mutex<TrainScheduler>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl Learning {
    /// Opens (creating if needed) the model and event store in `folder`.
    pub fn open(folder: &Path) -> Result<Self, StoreError> {
        let model = SortAdaptiveModel::new(folder);
        let store = Arc::new(SortEventStore::new(folder)?);
        let scheduler = Arc::new(Mutex::new(TrainScheduler::new(true)));
        let nudge = Arc::clone(&scheduler);
        let observer = SortObserver::new(Arc::new(model.clone()), store.clone(), move || lock(&nudge).request_immediate_retrain());
        let manager = SortLearningManager::new(Arc::new(model.clone()), store.clone());
        Ok(Learning { model, store, observer, manager, scheduler })
    }

    /// Retrains both heads on a background thread when due and `enabled`.
    pub fn train_if_due(&self, enabled: bool) {
        let mut scheduler = lock(&self.scheduler);
        scheduler.set_enabled(enabled);
        let (model, store) = (self.model.clone(), Arc::clone(&self.store));
        scheduler.train_if_due(now_secs(), move || {
            model.train_risk(store.get_risk_training_data(TRAINING_SAMPLES).unwrap_or_default(), true);
            model.train_items(store.get_item_training_data(TRAINING_SAMPLES).unwrap_or_default(), true);
        });
    }

    /// Remembers where `plan` puts every item, by catalog id, for [`Self::check_corrections`].
    pub fn register_plan(&self, session_id: &str, plan: &SortPlan, character: &Character) {
        let item_ids: HashMap<u64, &str> = character.items.iter().map(|i| (i.unique_id, i.item_id.as_str())).collect();
        let positions = plan
            .positions
            .iter()
            .filter_map(|(id, loc)| Some((item_ids.get(id)?.to_string(), (i64::from(loc.cell.x), i64::from(loc.cell.y)))))
            .collect();
        self.manager.register_pending_plan(session_id, positions, HashMap::new());
    }

    /// Compares `character`'s fresh stash `stash_id` with the last registered plan and records every
    /// item the player moved since. Returns how many.
    pub fn check_corrections(&self, character: &Character, stash_id: u32) -> usize {
        let width = grid_size(stash_id).map_or(1, |(w, _)| w);
        let cells: Vec<(String, i64, i64)> = character
            .stash(stash_id)
            .into_iter()
            .filter_map(|item| {
                let (x, y) = slot_cell(item.slot_id?, width);
                Some((item.item_id, i64::from(x), i64::from(y)))
            })
            .collect();
        let items: Vec<ItemPosition> = cells.iter().map(|(item_id, x, y)| ItemPosition { item_id, x: *x, y: *y }).collect();
        self.manager.check_corrections(&items)
    }

    /// The player's verdict on a finished sort.
    pub fn record_feedback(&self, session_id: &str, success: bool, note: Option<&str>) -> bool {
        self.observer.record_user_feedback(session_id, success, note)
    }
}
