//! The run loop against a simulated game: every move lands and is verified until the stash matches
//! the plan, and every reason a run must stop does stop it, before or between drags.

mod run_fakes;

use std::collections::HashMap;
use std::time::Duration;

use input::Cancel;
use run_fakes::*;
use sorter::plan::{intersects, Cell, Move, SortPlan};
use sorter::run::{LayoutCheck, RunObserver, RunPhase, StepReport, StopReason, Timing};
use state::stash::{BAG, STORAGE};

/// Every phase, layout check and finished step, in order.
#[derive(Default)]
struct Recorder {
    phases: Vec<RunPhase>,
    checks: Vec<LayoutCheck>,
    steps: Vec<StepReport>,
}

impl RunObserver for Recorder {
    fn phase(&mut self, phase: RunPhase) {
        self.phases.push(phase);
    }
    fn layout_checked(&mut self, check: &LayoutCheck) {
        self.checks.push(*check);
    }
    fn step_finished(&mut self, report: &StepReport) {
        self.steps.push(report.clone());
    }
}

fn planned_layout(plan: &SortPlan) -> HashMap<u64, Cell> {
    plan.positions.iter().map(|(&id, loc)| (id, loc.cell)).collect()
}

/// The 1x1 items of [`scattered`].
const SMALL_ITEMS: [u64; 3] = [1, 4, 5];

/// Index and item of the first relocation of a 1x1 item to a different cell (a retry is only ever
/// made when the old and new spots don't overlap).
fn first_clean_relocation(plan: &SortPlan) -> (usize, u64) {
    plan.moves
        .iter()
        .enumerate()
        .find_map(|(index, planned)| match *planned {
            Move::Relocate { unique_id, from, to }
                if SMALL_ITEMS.contains(&unique_id) && (from.inventory_id != to.inventory_id || !intersects(from.cell, 1, 1, to.cell, 1, 1)) =>
            {
                Some((index, unique_id))
            }
            _ => None,
        })
        .expect("the scattered stash relocates a small item")
}

#[test]
fn a_plan_runs_until_the_game_shows_the_sorted_stash() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    let other_tab_before = game.layout(OTHER_TAB);
    let mut recorder = Recorder::default();

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut recorder);

    // Assert
    assert!(!plan.moves.is_empty());
    assert!(report.is_complete(), "{report:?}");
    assert_eq!((report.done, report.verified, report.failed), (plan.moves.len(), plan.moves.len(), 0));
    assert_eq!(game.layout(STORAGE), planned_layout(&plan));
    assert_eq!(game.layout(BAG), HashMap::from([(8, Cell::new(9, 0))]), "the bag's own item stays put");
    assert_eq!(game.layout(OTHER_TAB), other_tab_before, "other tabs are never touched");
    let (downs, ups) = game.button_counts();
    assert_eq!(downs, ups, "every press is released");
    assert_eq!(recorder.phases, vec![RunPhase::FocusingGame, RunPhase::CheckingStash, RunPhase::Moving]);
    assert!(recorder.steps.iter().all(|s| s.verified && s.attempts == 1));
}

#[test]
fn a_move_that_never_lands_is_retried_once_then_stops_the_run() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let (index, stuck) = first_clean_relocation(&plan);
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    game.state().stuck.insert(stuck);
    let mut recorder = Recorder::default();

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut recorder);

    // Assert
    assert!(matches!(report.stop, Some(StopReason::NotVerified { step, .. }) if step == index), "{report:?}");
    assert_eq!((report.done, report.failed, report.retried), (index + 1, 1, 1));
    let last = recorder.steps.last().expect("the failed move is reported");
    assert_eq!((last.unique_id, last.attempts, last.verified), (stuck, 2, false));
    assert_eq!(recorder.steps.len(), index + 1, "nothing is dragged after the failed move");
}

#[test]
fn a_move_that_lands_on_the_second_drag_lets_the_run_finish() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let (_, flaky) = first_clean_relocation(&plan);
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    game.state().flaky.insert(flaky);
    let mut recorder = Recorder::default();

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut recorder);

    // Assert
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.retried, 1);
    assert!(recorder.steps.iter().any(|s| s.unique_id == flaky && s.attempts == 2 && s.verified));
    assert_eq!(game.layout(STORAGE), planned_layout(&plan));
}

#[test]
fn a_move_the_game_takes_back_stops_the_run_before_the_next_drag() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let (index, refused) = first_clean_relocation(&plan);
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    game.state().snap_back.insert(refused);
    let mut recorder = Recorder::default();

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut recorder);

    // Assert
    assert!(matches!(report.stop, Some(StopReason::MoveUndone { step, .. }) if step == index), "{report:?}");
    assert_eq!(recorder.steps.len(), index + 1, "nothing is dragged after the move the game took back");
    assert!(recorder.steps.last().is_some_and(|s| s.unique_id == refused && s.verified));
}

#[test]
fn another_tab_on_screen_is_switched_by_clicking_the_stash_tab() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), OTHER_TAB);
    let mut recorder = Recorder::default();

    // Act
    let report = run(&game, &character, &plan, &config(Some(TAB_POINT)), &Cancel::new(), &mut recorder);

    // Assert
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(game.state().presses.first(), Some(&TAB_POINT), "the tab is clicked before any drag");
    assert_eq!(game.layout(STORAGE), planned_layout(&plan));
    assert!(recorder.phases.contains(&RunPhase::OpeningTab));
    assert_eq!(recorder.checks.len(), 2);
}

#[test]
fn another_tab_on_screen_with_no_known_tab_stops_before_any_click() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), OTHER_TAB);
    let other_tab_before = game.layout(OTHER_TAB);

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut ());

    // Assert
    assert_eq!(report.stop, Some(StopReason::LayoutMismatch));
    assert_eq!(report.started, 0);
    assert_eq!(game.button_counts(), (0, 0), "nothing is ever pressed on a stash that doesn't match");
    assert_eq!(game.layout(OTHER_TAB), other_tab_before);
}

#[test]
fn a_stash_the_player_rearranged_since_the_last_update_stops_before_any_click() {
    // Arrange: in the game, both shields and a dagger were moved by hand after the app last heard.
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    for (id, cell) in [(2, Cell::new(8, 14)), (7, Cell::new(10, 18)), (3, Cell::new(0, 18))] {
        let mut state = game.state();
        let item = state.items.iter_mut().find(|i| i.unique_id == id).expect("item is in the game");
        item.cell = cell;
    }

    // Act
    let report = run(&game, &character, &plan, &config(Some(TAB_POINT)), &Cancel::new(), &mut ());

    // Assert
    assert_eq!(report.stop, Some(StopReason::LayoutMismatch));
    assert_eq!(report.started, 0);
    let presses = game.state().presses.clone();
    assert!(presses.iter().all(|&p| p == TAB_POINT), "only the tab may be clicked: {presses:?}");
}

#[test]
fn moving_the_mouse_stops_the_run() {
    // Arrange: after 40 cursor moves the player pushes the mouse 80 px away.
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    game.state().drift_after_moves = Some((40, (80, 0)));

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut ());

    // Assert
    assert_eq!(report.stop, Some(StopReason::MouseMoved));
    assert!(report.done < report.total);
    let (downs, ups) = game.button_counts();
    assert!(ups >= downs, "a stopped drag releases the button");
}

#[test]
fn losing_focus_stops_the_run_once_the_grace_period_is_over() {
    // Arrange: the game loses focus at the first drop; moves are slowed so the run outlasts the
    // 600 ms grace.
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    game.state().lose_focus_after_drops = Some(1);
    let mut slow = config(None);
    slow.timing = Timing { before_capture: Duration::from_millis(60), after_drag: Duration::from_millis(120), ..Timing::INSTANT };

    // Act
    let report = run(&game, &character, &plan, &slow, &Cancel::new(), &mut ());

    // Assert
    assert_eq!(report.stop, Some(StopReason::FocusLost));
    assert!(report.done < report.total);
}

#[test]
fn stopping_mid_run_stops_at_once_and_releases_the_button() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    let cancel = Cancel::new();
    game.state().cancel_after_drops = Some((3, cancel.clone()));

    // Act
    let report = run(&game, &character, &plan, &config(None), &cancel, &mut ());

    // Assert
    assert_eq!(report.stop, Some(StopReason::Cancelled));
    assert!(report.done < report.total);
    let (downs, ups) = game.button_counts();
    assert!(ups >= downs, "a stopped drag releases the button");
}

#[test]
fn a_run_stopped_before_it_starts_sends_nothing() {
    // Arrange
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    let cancel = Cancel::new();
    cancel.cancel();

    // Act
    let report = run(&game, &character, &plan, &config(None), &cancel, &mut ());

    // Assert
    assert_eq!(report.stop, Some(StopReason::Cancelled));
    assert!(game.state().events.is_empty());
}

#[test]
fn no_game_running_stops_the_run() {
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let game = FakeGame::new(&character, &catalog(), STORAGE);
    game.state().running = false;

    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut ());

    assert_eq!(report.stop, Some(StopReason::GameNotFound));
    assert_eq!(game.button_counts(), (0, 0));
}

#[test]
fn partial_stacks_merge_in_the_game() {
    // Arrange: three potion stacks of 2 (stack size 5) and a ring.
    let character = character(vec![
        owned(1, "Potion_1", 2, STORAGE, 40),
        owned(2, "Potion_1", 2, STORAGE, 90),
        owned(3, "Potion_1", 2, STORAGE, 13),
        owned(4, "Ring_1", 1, STORAGE, 5),
    ]);
    let plan = plan_for(&character, true, Vec::new());
    assert!(plan.moves.iter().any(|m| matches!(m, Move::StackInto { .. })));
    let game = FakeGame::new(&character, &catalog(), STORAGE);

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut ());

    // Assert
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(game.layout(STORAGE), planned_layout(&plan));
}

#[test]
fn items_come_over_from_the_bag() {
    // Arrange
    let character = character(vec![
        owned(1, "Shield_1", 1, STORAGE, 50),
        owned(2, "Gem_1", 1, BAG, 3),
        owned(3, "Dagger_1", 1, BAG, 0),
    ]);
    let plan = plan_for(&character, false, vec![2, 3]);
    let game = FakeGame::new(&character, &catalog(), STORAGE);

    // Act
    let report = run(&game, &character, &plan, &config(None), &Cancel::new(), &mut ());

    // Assert
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(game.layout(STORAGE), planned_layout(&plan));
    assert!(game.layout(BAG).is_empty());
}

#[test]
fn an_already_sorted_stash_leaves_the_game_alone() {
    // Arrange: plan again from the layout the first plan produces. Only distinct items: the planner
    // may swap two identical items on a second plan (their tie order isn't stable).
    let character = character(vec![
        owned(1, "Shield_1", 1, STORAGE, 64),
        owned(2, "Dagger_1", 1, STORAGE, 101),
        owned(3, "Gem_1", 1, STORAGE, 7),
        owned(4, "Ring_1", 1, STORAGE, 150),
    ]);
    let plan = plan_for(&character, false, Vec::new());
    let mut sorted = character.clone();
    for item in sorted.items.iter_mut().filter(|i| i.inventory_id == STORAGE) {
        let cell = plan.positions[&item.unique_id].cell;
        item.slot_id = Some(cell.y * 12 + cell.x);
    }
    let again = plan_for(&sorted, false, Vec::new());
    let game = FakeGame::new(&sorted, &catalog(), STORAGE);

    // Act
    let report = run(&game, &sorted, &again, &config(None), &Cancel::new(), &mut ());

    // Assert
    assert!(again.moves.is_empty(), "{:?}", again.moves);
    assert!(report.is_complete());
    assert!(game.state().events.is_empty());
    assert!(!game.state().focused, "the game isn't even brought forward");
}
