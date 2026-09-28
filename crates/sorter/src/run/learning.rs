//! Feeding a run into sort learning: the counts a session starts from, the plan metrics the risk
//! model trains on, and one outcome per move. Port of the feedback calls spread through
//! `StashSorter` (`begin_session`'s features in `__init__`, `_metric_set` in `_build_sort_plan`,
//! `_record_move_outcome`).

use std::collections::{HashMap, HashSet};

use super::report::{RunObserver, RunPhase, StepReport};
use super::steps::{Board, RunStep, RunSteps, StepKind};
use super::LayoutCheck;
use crate::learn::{SortEventStore, SortFeedbackHandle};
use crate::plan::SortPlan;

/// The stash and bag counts a sort session starts from, for
/// [`crate::learn::SortObserver::begin_session`]. Python: `StashSorter.__init__`'s
/// `base_features`.
pub fn session_features(board: &Board) -> HashMap<String, f64> {
    let cells = |inventory_id: u32| board.grid(inventory_id).map_or((0, 0), |g| (g.width() * g.height(), g.count_free_cells()));
    let (stash_total, stash_free) = cells(board.stash_id());
    let (bag_total, bag_free) = cells(board.bag_id());
    let mut features = HashMap::from([
        ("stash_total_cells".to_string(), f64::from(stash_total)),
        ("stash_occupied_cells".to_string(), f64::from(stash_total - stash_free)),
        ("inventory_total_cells".to_string(), f64::from(bag_total)),
        ("inventory_free_cells".to_string(), f64::from(bag_free)),
    ]);
    if let Some(largest) = board.items_on(board.stash_id()).map(|(_, _, w, h)| w * h).max() {
        features.insert("largest_item_area".to_string(), f64::from(largest));
    }
    features
}

/// Plan metrics for [`SortFeedbackHandle::set_metric`], named as the risk model reads them.
/// Python set `plan_size`, `largest_item_area` and the needing-move counts in `_build_sort_plan`;
/// parking drags into the bag stand in for its `park_attempts` and `buffered_items`.
pub fn plan_metrics(steps: &RunSteps, plan: &SortPlan) -> Vec<(&'static str, f64)> {
    let initial = steps.initial();
    let needing_move = plan.positions.iter().filter(|(&id, &target)| initial.location(id) != Some(target)).count();
    let largest = initial.items_on(initial.stash_id()).map(|(_, _, w, h)| w * h).max().unwrap_or(0);
    let parks: Vec<&RunStep> = steps.steps().iter().filter(|s| s.to.inventory_id == steps.bag_id()).collect();
    let buffered: HashSet<u64> = parks.iter().map(|s| s.unique_id).collect();
    let merges = steps.steps().iter().filter(|s| matches!(s.kind, StepKind::Stack { .. })).count();
    vec![
        ("plan_size", plan.order.len() as f64),
        ("largest_item_area", f64::from(largest)),
        ("plan_items_needing_move", needing_move as f64),
        ("plan_items_already_placed", plan.order.len().saturating_sub(needing_move) as f64),
        ("planned_moves", steps.len() as f64),
        ("park_attempts", parks.len() as f64),
        ("buffered_items", buffered.len() as f64),
        ("stack_merges", merges as f64),
    ]
}

/// Where per-move outcomes go. [`SortEventStore`] keeps them as `MOVE_OUTCOME` events.
pub trait MoveOutcomeSink: Send + Sync {
    fn record_move(&self, session_id: &str, features: &HashMap<String, f64>, verified: bool, attempts: u32, item_id: &str, reason: Option<&str>);
}

impl MoveOutcomeSink for SortEventStore {
    fn record_move(&self, session_id: &str, features: &HashMap<String, f64>, verified: bool, attempts: u32, item_id: &str, reason: Option<&str>) {
        // One lost training sample must never stop a sort, as in Python's `except Exception: pass`.
        let _ = self.record_move_outcome(session_id, features, verified, attempts, Some(item_id), reason);
    }
}

/// A [`RunObserver`] that counts moves on the session's feedback handle, records each move's
/// outcome, and passes everything on to `inner`.
pub struct LearningRecorder<'a> {
    handle: &'a mut SortFeedbackHandle,
    moves: Option<&'a dyn MoveOutcomeSink>,
    inner: &'a mut dyn RunObserver,
}

impl<'a> LearningRecorder<'a> {
    pub fn new(handle: &'a mut SortFeedbackHandle, moves: Option<&'a dyn MoveOutcomeSink>, inner: &'a mut dyn RunObserver) -> Self {
        LearningRecorder { handle, moves, inner }
    }
}

impl RunObserver for LearningRecorder<'_> {
    fn phase(&mut self, phase: RunPhase) {
        self.inner.phase(phase);
    }

    fn layout_checked(&mut self, check: &LayoutCheck) {
        self.inner.layout_checked(check);
    }

    fn step_started(&mut self, step: &RunStep, index: usize, total: usize) {
        self.inner.step_started(step, index, total);
    }

    fn step_finished(&mut self, report: &StepReport) {
        self.handle.increment("moves_executed", 1.0);
        self.handle.increment(if report.verified { "moves_verified" } else { "moves_failed" }, 1.0);
        if report.attempts > 1 {
            self.handle.increment("moves_retried", 1.0);
        }
        if let Some(sink) = self.moves {
            let reason = (!report.verified).then_some("not_verified");
            sink.record_move(self.handle.session_id(), &move_features(report), report.verified, report.attempts, &report.item_id, reason);
        }
        self.inner.step_finished(report);
    }
}

/// `MOVE_FEATURE_NAMES` for one move. Python: `StashSorter._record_move_outcome`.
fn move_features(report: &StepReport) -> HashMap<String, f64> {
    HashMap::from([
        ("item_area".to_string(), f64::from(report.item_area)),
        ("move_distance_grid".to_string(), f64::from(report.distance_cells)),
        ("source_stash_type".to_string(), f64::from(report.from_inventory)),
        ("dest_stash_type".to_string(), f64::from(report.to_inventory)),
        ("current_delay".to_string(), report.delay_s),
        ("move_index".to_string(), report.index as f64),
        ("consecutive_successes".to_string(), f64::from(report.streak_before)),
    ])
}
