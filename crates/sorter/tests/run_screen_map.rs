//! Plan cells to screen pixels: centres, probes, grid captures, and where the cursor rests.

mod run_fakes;

use run_fakes::*;
use sorter::plan::{Cell, Location, World};
use sorter::run::{Board, GridGeometry, RunStep, RunSteps, ScreenMap, StepKind};
use state::stash::{BAG, STORAGE};
use state::LOCKED_SEASONAL_STASH;

const HD: GridGeometry = GridGeometry { stash_origin: (1378.0, 199.0), bag_origin: (690.0, 626.0), jump: 40.5 };

/// The cell under a screen point, on either grid of the test map.
fn cell_at(point: (i32, i32)) -> Option<(u32, Cell)> {
    [(GEOMETRY.stash_origin, (12, 20), STORAGE), (GEOMETRY.bag_origin, (10, 5), BAG)].into_iter().find_map(|((ox, oy), (w, h), id)| {
        let (fx, fy) = ((f64::from(point.0) - ox) / JUMP, (f64::from(point.1) - oy) / JUMP);
        (fx >= 0.0 && fy >= 0.0 && fx < f64::from(w) && fy < f64::from(h)).then(|| (id, Cell::new(fx as u32, fy as u32)))
    })
}

fn chebyshev(cell: Cell, origin: Cell, width: u32, height: u32) -> u32 {
    let gap = |c: u32, start: u32, len: u32| if c < start { start - c } else { c.saturating_sub(start + len - 1) };
    gap(cell.x, origin.x, width).max(gap(cell.y, origin.y, height))
}

#[test]
fn cell_centres_follow_the_drag_arithmetic() {
    let map = map();
    // A 2x2 item at stash cell (1, 1): 300 + 10 + 10 = 320, 40 + 10 + 10 = 60.
    assert_eq!(map.center(Location::new(STORAGE, Cell::new(1, 1)), 2, 2), Some((320.0, 60.0)));
    assert_eq!(map.center(Location::new(BAG, Cell::new(0, 0)), 1, 1), Some((45.0, 125.0)));
    let endpoint = map.endpoint(Location::new(BAG, Cell::new(3, 2)), 1, 2).expect("the bag is on screen");
    assert_eq!((endpoint.origin, endpoint.width, endpoint.height), (GEOMETRY.bag_origin, 1, 2));
}

#[test]
fn the_locked_stash_and_other_tabs_have_no_place_on_screen() {
    let map = map();
    assert_eq!(map.center(Location::new(LOCKED_SEASONAL_STASH, Cell::new(0, 0)), 1, 1), None);
    assert!(map.endpoint(Location::new(OTHER_TAB, Cell::new(0, 0)), 1, 1).is_none());
    assert!(map.grid_region(LOCKED_SEASONAL_STASH).is_none());
    assert!(map.probe(Location::new(LOCKED_SEASONAL_STASH, Cell::new(2, 2)), 1, 1).is_none());
}

#[test]
fn a_probe_is_a_square_centred_on_the_item_that_grows_with_the_cells() {
    let map = map();
    assert_eq!(map.probe_side(), 12, "never smaller than 12 px");
    assert_eq!(map.probe(Location::new(STORAGE, Cell::new(0, 0)), 1, 1), Some((299, 39, 311, 51)));
    let four_k = ScreenMap::new(GridGeometry { stash_origin: (2756.0, 398.0), bag_origin: (1380.0, 1252.0), jump: 81.0 }, STORAGE, BAG);
    assert_eq!(four_k.probe_side(), 41);
}

#[test]
fn every_cell_interior_has_one_size_and_lies_inside_the_grid_capture() {
    let map = ScreenMap::new(HD, STORAGE, BAG);
    for (inventory_id, (w, h)) in [(STORAGE, (12, 20)), (BAG, (10, 5))] {
        let (left, top, right, bottom) = map.grid_region(inventory_id).expect("grid on screen");
        let (width, height) = ((right - left) as usize, (bottom - top) as usize);
        for y in 0..h {
            for x in 0..w {
                let (l, t, r, b) = map.cell_interior(inventory_id, Cell::new(x, y)).expect("cell on the grid");
                assert_eq!((r - l, b - t), (20, 20));
                assert!(r <= width && b <= height, "cell ({x}, {y}) of {inventory_id} leaves the capture");
            }
        }
        assert!(map.cell_interior(inventory_id, Cell::new(w, 0)).is_none());
    }
}

#[test]
fn the_neutral_point_sits_between_the_bag_and_the_stash() {
    assert_eq!(map().neutral_point(), (220, 145));
    let hd = ScreenMap::new(HD, STORAGE, BAG);
    assert_eq!(hd.neutral_point(), (1237, 727));
    assert!(cell_at(map().neutral_point()).is_none());
}

#[test]
fn the_cursor_parks_on_a_free_cell_away_from_both_ends_of_every_move() {
    let character = scattered();
    let plan = plan_for(&character, false, Vec::new());
    let steps = RunSteps::new(&character, &catalog(), STORAGE, &plan).expect("plan fits");
    let map = map();
    let mut board = steps.initial().clone();
    for step in steps.steps() {
        let drop = map.center(step.to, step.to_width, step.to_height).expect("on screen");
        let park = map.parking_point(&board, step, drop);
        let (inventory_id, cell) = cell_at(park).expect("parks on a grid while there is room");
        assert!(!board.is_occupied(inventory_id, cell), "parked on an item");
        for (loc, w, h) in [(step.from, step.width, step.height), (step.to, step.to_width, step.to_height)] {
            if loc.inventory_id == inventory_id {
                assert!(chebyshev(cell, loc.cell, w, h) >= 2, "parked next to a probed item");
            }
        }
        board.apply(step).expect("steps follow each other");
    }
}

#[test]
fn with_no_free_cell_the_cursor_parks_outside_the_grids() {
    let mut items: Vec<_> = (0..240).map(|slot| owned(1000 + u64::from(slot), "Ring_1", 1, STORAGE, slot)).collect();
    items.extend((0..50).map(|slot| owned(2000 + u64::from(slot), "Gem_1", 1, BAG, slot)));
    let character = character(items);
    let world = World::build(&character, &catalog(), STORAGE).expect("storage loads");
    let board = Board::from_world(&world);
    let step = RunStep {
        unique_id: 1000,
        item_id: "Ring_1".into(),
        name: "Ring".into(),
        kind: StepKind::Relocate,
        from: Location::new(STORAGE, Cell::new(0, 0)),
        to: Location::new(STORAGE, Cell::new(5, 5)),
        width: 1,
        height: 1,
        to_width: 1,
        to_height: 1,
    };
    let map = map();
    assert_eq!(map.parking_point(&board, &step, (350.0, 90.0)), map.neutral_point());
}
