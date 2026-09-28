//! What the Stash sorter page polls: the current or last run, move by move.

use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use sorter::run::{LayoutCheck, RunObserver, RunPhase, RunReport, RunStep, StepReport};

/// The run as the page shows it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SorterStatus {
    /// "idle", "running", "done" (every move made and verified) or "stopped".
    pub state: &'static str,
    pub character_id: Option<String>,
    pub stash_id: Option<u32>,
    pub stash_label: Option<String>,
    /// What the run is doing between moves ("Checking the stash on screen").
    pub phase: Option<String>,
    /// The item being moved right now.
    pub current: Option<String>,
    /// Moves dragged and checked, of `total`.
    pub done: usize,
    pub total: usize,
    pub verified: usize,
    pub failed: usize,
    pub retried: usize,
    /// Why the run stopped early, for the player.
    pub stopped_reason: Option<String>,
    /// The learning session, for "did it work?" feedback. None when learning is off.
    pub session_id: Option<String>,
    pub feedback_sent: bool,
    /// Whether Ctrl+F12 stops this run.
    pub hotkey: bool,
    /// Another feature drives the mouse right now ("The auto lister"), so Sort must wait.
    pub busy_with: Option<String>,
    /// Unix seconds when the run ended.
    pub finished_at: Option<u64>,
}

impl SorterStatus {
    pub fn idle() -> Self {
        SorterStatus { state: "idle", ..SorterStatus::default() }
    }

    pub fn is_running(&self) -> bool {
        self.state == "running"
    }

    /// Counts and outcome from a finished run.
    pub fn finish(&mut self, report: &RunReport, finished_at: u64) {
        self.state = if report.is_complete() { "done" } else { "stopped" };
        self.phase = None;
        self.current = None;
        self.done = report.done;
        self.total = report.total;
        self.verified = report.verified;
        self.failed = report.failed;
        self.retried = report.retried;
        self.stopped_reason = report.stop.as_ref().map(|reason| reason.message());
        self.finished_at = Some(finished_at);
    }
}

pub fn lock(status: &Mutex<SorterStatus>) -> MutexGuard<'_, SorterStatus> {
    status.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keeps [`SorterStatus`] current while a run goes.
pub struct StatusObserver {
    status: Arc<Mutex<SorterStatus>>,
}

impl StatusObserver {
    pub fn new(status: Arc<Mutex<SorterStatus>>) -> Self {
        StatusObserver { status }
    }
}

impl RunObserver for StatusObserver {
    fn phase(&mut self, phase: RunPhase) {
        lock(&self.status).phase = Some(phase.label().to_string());
    }

    fn layout_checked(&mut self, check: &LayoutCheck) {
        log::info!(
            "stash check {:?}: {}/{} empty cells look empty, {}/{} occupied cells look occupied",
            check.verdict,
            check.empty_seen,
            check.empty_total,
            check.occupied_seen,
            check.occupied_total
        );
    }

    fn step_started(&mut self, step: &RunStep, _index: usize, total: usize) {
        let mut status = lock(&self.status);
        status.current = Some(step.name.clone());
        status.total = total;
    }

    fn step_finished(&mut self, report: &StepReport) {
        let mut status = lock(&self.status);
        status.done += 1;
        if report.verified {
            status.verified += 1;
        } else {
            status.failed += 1;
            log::warn!(
                "move {} ({}) not confirmed: source changed {:.1}, destination changed {:.1}",
                report.index + 1,
                report.item_name,
                report.source_diff,
                report.dest_diff
            );
        }
        if report.attempts > 1 {
            status.retried += 1;
        }
    }
}
