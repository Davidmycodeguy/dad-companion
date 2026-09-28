//! One move at a time: park the cursor, capture, drag, capture again, and decide. Port of the
//! replay loop in `StashSorter._replay_planned_moves` with `MoveVerifier`'s `pre_capture` /
//! `post_verify` around every drag.

use std::time::Duration;

use input::move_from_to_reliable;
use screen::{Frame, Region};

use super::{input_error, Run};
use crate::plan::{intersects, Location};
use crate::run::report::{RunPhase, StepReport, StopReason};
use crate::run::screen_map::footprints;
use crate::run::steps::{RunStep, RunSteps};
use crate::run::verify::{compare_probes, MoveCheck};

/// What a confirmed move left on screen: its two probes as last captured.
pub(super) struct Landing {
    index: usize,
    item: String,
    footprints: [(Location, u32, u32); 2],
    probes: (Region, Region),
    after: (Frame, Frame),
}

/// A step whose ends are not on the stash or bag grid (never produced by [`RunSteps::new`]).
fn off_grid() -> StopReason {
    StopReason::Refused("A move lies outside the stash and bag on screen.".into())
}

impl Run<'_> {
    pub(super) fn move_all(&mut self, steps: &RunSteps) -> Result<(), StopReason> {
        self.observer.phase(RunPhase::Moving);
        let total = steps.len();
        for (index, step) in steps.steps().iter().enumerate() {
            self.checkpoint()?;
            self.observer.step_started(step, index, total);
            let report = self.move_one(index, step)?;
            self.report.done += 1;
            if report.attempts > 1 {
                self.report.retried += 1;
            }
            if report.verified {
                self.report.verified += 1;
            } else {
                self.report.failed += 1;
            }
            self.observer.step_finished(&report);
            if !report.verified {
                return Err(StopReason::NotVerified { step: index, item: step.name.clone() });
            }
            self.board.apply(step).map_err(StopReason::Refused)?;
        }
        self.wait(self.config.timing.recheck)?;
        self.check_landing()
    }

    /// Looks again at the last move's probes: a change since it was confirmed means the game put
    /// the item back (or something else moved), and the board no longer matches the game.
    fn check_landing(&mut self) -> Result<(), StopReason> {
        let Some(landing) = self.landing.take() else { return Ok(()) };
        let now = (self.grab(landing.probes.0)?, self.grab(landing.probes.1)?);
        let drift = compare_probes((&landing.after.0, &landing.after.1), (&now.0, &now.1));
        if drift.source_changed() || drift.dest_changed() {
            return Err(StopReason::MoveUndone { step: landing.index, item: landing.item });
        }
        Ok(())
    }

    /// Drags one move and confirms it: a first look after the drag, a second, slower look for a
    /// game that draws late, then one more drag (only when it can't grab the item off-centre) and
    /// a last look. Every look compares with the captures taken before the first drag.
    fn move_one(&mut self, index: usize, step: &RunStep) -> Result<StepReport, StopReason> {
        let config = self.config;
        let map = &config.map;
        let source = map.probe(step.from, step.width, step.height).ok_or_else(off_grid)?;
        let dest = map.probe(step.to, step.to_width, step.to_height).ok_or_else(off_grid)?;
        let drop = map.center(step.to, step.to_width, step.to_height).ok_or_else(off_grid)?;
        let mut avoid = footprints(step).to_vec();
        avoid.extend(self.landing.iter().flat_map(|landing| landing.footprints));
        let park = map.parking_point_avoiding(&self.board, &avoid, drop);
        let streak_before = self.speed.streak();

        self.travel(park)?;
        self.wait(config.timing.before_capture)?;
        self.check_landing()?;
        let before = (self.grab(source)?, self.grab(dest)?);

        self.report.started += 1;
        let mut attempts = 1;
        let mut delay = self.speed.delay();
        self.drag(step, delay)?;
        let (mut check, mut after) = self.look(park, (source, dest), &before, config.timing.after_drag)?;
        if !check.verified() {
            (check, after) = self.look(park, (source, dest), &before, config.timing.recheck)?;
        }
        if !check.verified() && retry_is_safe(step) {
            self.speed.record(false);
            attempts = 2;
            delay = self.speed.delay();
            self.drag(step, delay)?;
            (check, after) = self.look(park, (source, dest), &before, config.timing.recheck)?;
        }
        self.speed.record(check.verified());
        if check.verified() {
            self.landing = Some(Landing { index, item: step.name.clone(), footprints: footprints(step), probes: (source, dest), after });
        }
        Ok(step_report(index, step, attempts, check, delay, streak_before))
    }

    /// Parks the cursor, waits `settle`, captures both probes again and compares with `before`.
    /// Returns the comparison and the new captures.
    fn look(
        &mut self,
        park: (i32, i32),
        probes: (Region, Region),
        before: &(Frame, Frame),
        settle: Duration,
    ) -> Result<(MoveCheck, (Frame, Frame)), StopReason> {
        self.travel(park)?;
        self.wait(settle)?;
        let after = (self.grab(probes.0)?, self.grab(probes.1)?);
        Ok((compare_probes((&before.0, &before.1), (&after.0, &after.1)), after))
    }

    fn grab(&self, region: Region) -> Result<Frame, StopReason> {
        self.machine.grab(region).map_err(StopReason::Capture)
    }

    fn drag(&mut self, step: &RunStep, delay: Duration) -> Result<(), StopReason> {
        let config = self.config;
        let map = &config.map;
        let start = map.endpoint(step.from, step.width, step.height).ok_or_else(off_grid)?;
        let end = map.endpoint(step.to, step.to_width, step.to_height).ok_or_else(off_grid)?;
        move_from_to_reliable(&mut self.sink, self.cancel, map.jump(), delay, start, end, &mut self.drag_state)
            .map_err(|err| input_error(self.safety, err))?;
        self.checkpoint()
    }
}

fn step_report(index: usize, step: &RunStep, attempts: u32, check: MoveCheck, delay: Duration, streak_before: u32) -> StepReport {
    StepReport {
        index,
        unique_id: step.unique_id,
        item_id: step.item_id.clone(),
        item_name: step.name.clone(),
        kind: step.kind,
        attempts,
        verified: check.verified(),
        source_diff: check.source_diff,
        dest_diff: check.dest_diff,
        delay_s: delay.as_secs_f64(),
        streak_before,
        distance_cells: step.distance_cells(),
        item_area: step.width * step.height,
        from_inventory: step.from.inventory_id,
        to_inventory: step.to.inventory_id,
    }
}

/// A second drag is safe unless the item's old and new spots overlap on the same grid: if the
/// first drag did move it, the second would grab it off-centre and drop it in the wrong place.
fn retry_is_safe(step: &RunStep) -> bool {
    step.from.inventory_id != step.to.inventory_id
        || !intersects(step.from.cell, step.width, step.height, step.to.cell, step.to_width, step.to_height)
}
