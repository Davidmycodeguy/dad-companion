//! Turning a plan into drags: sizes and names attached, the locked stash refused, and a plan that no
//! longer matches the stash refused before anything runs.

mod run_fakes;

use std::collections::HashMap;

use run_fakes::*;
use sorter::plan::{Cell, Location, Move, SortPlan};
use sorter::run::{RunStep, RunSteps, StepKind, StopReason};
use state::stash::STORAGE;
use state::LOCKED_SEASONAL_STASH;

fn relocate(unique_id: u64, from: (u32, Cell), to: (u32, Cell)) -> SortPlan {
    SortPlan { moves: vec![Move::Relocate { unique_id, from: Location::new(from.0, from.1), to: Location::new(to.0, to.1) }], ..SortPlan::default() }
}

#[test]
fn the_locked_seasonal_stash_is_refused_outright() {
    let err = RunSteps::new(&scattered(), &catalog(), LOCKED_SEASONAL_STASH, &SortPlan::default()).unwrap_err();
    assert!(matches!(err, StopReason::Refused(ref why) if why.contains("locked")), "{err:?}");
}

#[test]
fn a_move_into_or_out_of_the_locked_stash_is_refused() {
    // Ring 1 sits at slot 30 of the storage tab: cell (6, 2).
    let into = relocate(1, (STORAGE, Cell::new(6, 2)), (LOCKED_SEASONAL_STASH, Cell::new(0, 0)));
    assert!(matches!(RunSteps::new(&scattered(), &catalog(), STORAGE, &into), Err(StopReason::Refused(_))));
    let out_of = relocate(1, (LOCKED_SEASONAL_STASH, Cell::new(0, 0)), (STORAGE, Cell::new(0, 0)));
    assert!(matches!(RunSteps::new(&scattered(), &catalog(), STORAGE, &out_of), Err(StopReason::Refused(_))));
}

#[test]
fn a_move_to_a_tab_that_is_not_being_sorted_is_refused() {
    let plan = relocate(1, (STORAGE, Cell::new(6, 2)), (OTHER_TAB, Cell::new(5, 5)));
    let err = RunSteps::new(&scattered(), &catalog(), STORAGE, &plan).unwrap_err();
    assert!(matches!(err, StopReason::Refused(ref why) if why.contains("isn't being sorted")), "{err:?}");
}

#[test]
fn a_plan_that_no_longer_fits_the_stash_is_refused() {
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let moved = match plan.moves[0] {
        Move::Relocate { unique_id, .. } | Move::StackInto { unique_id, .. } => unique_id,
    };
    let mut changed = character.clone();
    changed.items.iter_mut().find(|i| i.unique_id == moved).expect("moved item").slot_id = Some(239);
    assert!(matches!(RunSteps::new(&changed, &catalog(), STORAGE, &plan), Err(StopReason::Refused(_))));
}

#[test]
fn steps_carry_sizes_names_and_merge_targets() {
    let character = character(vec![
        owned(1, "Potion_1", 2, STORAGE, 40),
        owned(2, "Potion_1", 2, STORAGE, 90),
        owned(3, "Potion_1", 2, STORAGE, 13),
        owned(4, "Shield_1", 1, STORAGE, 100),
    ]);
    let plan = plan_for(&character, true, Vec::new());
    let steps = RunSteps::new(&character, &catalog(), STORAGE, &plan).expect("plan fits");

    assert_eq!(steps.len(), plan.moves.len());
    let merge = steps.steps().iter().find(|s| matches!(s.kind, StepKind::Stack { .. })).expect("a merge step");
    assert_eq!((merge.name.as_str(), merge.width, merge.height, merge.to_width, merge.to_height), ("Potion", 1, 1, 1, 1));
    if let Some(shield) = steps.steps().iter().find(|s| s.unique_id == 4) {
        assert_eq!((shield.name.as_str(), shield.width, shield.height), ("Shield", 2, 2));
    }
}

#[test]
fn the_finished_board_is_the_planned_layout() {
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let steps = RunSteps::new(&character, &catalog(), STORAGE, &plan).expect("plan fits");
    let finished: HashMap<u64, Cell> = steps.finished().items_on(STORAGE).map(|(id, cell, _, _)| (id, cell)).collect();
    let planned: HashMap<u64, Cell> = plan.positions.iter().map(|(&id, loc)| (id, loc.cell)).collect();
    assert_eq!(finished, planned);
    assert_eq!(steps.initial().location(1), Some(Location::new(STORAGE, Cell::new(6, 2))));
}

#[test]
fn distance_counts_cells_along_both_axes() {
    let step = RunStep {
        unique_id: 1,
        item_id: "Ring_1".into(),
        name: "Ring".into(),
        kind: StepKind::Relocate,
        from: Location::new(STORAGE, Cell::new(1, 1)),
        to: Location::new(STORAGE, Cell::new(4, 3)),
        width: 1,
        height: 1,
        to_width: 1,
        to_height: 1,
    };
    assert_eq!(step.distance_cells(), 5);
}
