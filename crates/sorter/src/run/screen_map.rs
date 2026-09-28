//! Where the stash and bag grids are on screen: the plan's cells as physical pixels, the squares
//! the verifier captures, and where the cursor rests while it does. Python computed the same cell
//! centres inline in `macros.move_from_to_reliable` and `MoveVerifier._slot_screen_center`.

use input::{cell_centre, DragEndpoint, DragSlot, Point, ScreenLayout};
use screen::Region;
use state::grid_size;

use super::steps::{Board, RunStep};
use crate::plan::{Cell, Location};

/// Top-left corners of the stash and bag grids and the size of one cell, in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridGeometry {
    pub stash_origin: (f64, f64),
    pub bag_origin: (f64, f64),
    pub jump: f64,
}

impl GridGeometry {
    /// The stash (`layout.stash`) and inventory (`layout.inv`) grids of a resolved screen layout.
    pub fn from_layout(layout: &ScreenLayout) -> Self {
        GridGeometry { stash_origin: layout.stash, bag_origin: layout.inv, jump: layout.jump }
    }
}

/// Smallest verifier probe side, in pixels.
const MIN_PROBE_PX: f64 = 12.0;
/// A probe is this share of one cell wide, centred on the item (Python: a fixed 20 px square,
/// which is half a cell at 1080p).
const PROBE_SHARE: f64 = 0.5;
/// The inside of a cell used to recognise empty cells: this share is trimmed from every side, so
/// grid lines never count.
const INTERIOR_INSET: f64 = 0.25;
/// A parking cell keeps at least this many cells between the cursor and either end of the move.
const PARK_DISTANCE: u32 = 2;

/// The plan's two grids, the stash being sorted and the bag, mapped onto the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenMap {
    geometry: GridGeometry,
    stash_id: u32,
    bag_id: u32,
}

impl ScreenMap {
    pub fn new(geometry: GridGeometry, stash_id: u32, bag_id: u32) -> Self {
        ScreenMap { geometry, stash_id, bag_id }
    }

    pub fn geometry(&self) -> GridGeometry {
        self.geometry
    }

    pub fn jump(&self) -> f64 {
        self.geometry.jump
    }

    pub fn stash_id(&self) -> u32 {
        self.stash_id
    }

    pub fn bag_id(&self) -> u32 {
        self.bag_id
    }

    fn origin(&self, inventory_id: u32) -> Option<(f64, f64)> {
        if inventory_id == self.stash_id {
            Some(self.geometry.stash_origin)
        } else if inventory_id == self.bag_id {
            Some(self.geometry.bag_origin)
        } else {
            None
        }
    }

    /// Grid size in cells of the stash or the bag; `None` for anything else.
    pub fn size(&self, inventory_id: u32) -> Option<(u32, u32)> {
        self.origin(inventory_id).and(grid_size(inventory_id))
    }

    /// Screen centre of a `width`x`height` item whose top-left cell is `loc`.
    pub fn center(&self, loc: Location, width: u32, height: u32) -> Option<(f64, f64)> {
        let origin = self.origin(loc.inventory_id)?;
        Some(cell_centre(origin, self.geometry.jump, point(loc.cell), width as i32, height as i32))
    }

    /// The same item as a drag endpoint for `input::move_from_to_reliable`.
    pub fn endpoint(&self, loc: Location, width: u32, height: u32) -> Option<DragEndpoint> {
        let origin = self.origin(loc.inventory_id)?;
        Some(DragEndpoint {
            slot: DragSlot { stash_id: u64::from(loc.inventory_id), cell: point(loc.cell) },
            origin,
            width: width as i32,
            height: height as i32,
        })
    }

    /// Side of a verifier probe, in pixels.
    pub fn probe_side(&self) -> i32 {
        (self.geometry.jump * PROBE_SHARE).max(MIN_PROBE_PX).round() as i32
    }

    /// The square the verifier captures for an item: centred on it, half a cell wide.
    pub fn probe(&self, loc: Location, width: u32, height: u32) -> Option<Region> {
        let (cx, cy) = self.center(loc, width, height)?;
        let side = self.probe_side();
        let left = (cx - f64::from(side) / 2.0).round() as i32;
        let top = (cy - f64::from(side) / 2.0).round() as i32;
        Some((left, top, left + side, top + side))
    }

    /// The pixels of a whole grid, for one capture.
    pub fn grid_region(&self, inventory_id: u32) -> Option<Region> {
        let (ox, oy) = self.origin(inventory_id)?;
        let (w, h) = self.size(inventory_id)?;
        let jump = self.geometry.jump;
        Some((ox.floor() as i32, oy.floor() as i32, (ox + f64::from(w) * jump).ceil() as i32, (oy + f64::from(h) * jump).ceil() as i32))
    }

    /// The inside of one cell as `(left, top, right, bottom)` within a capture of
    /// [`Self::grid_region`]. Every cell's interior has the same size.
    pub fn cell_interior(&self, inventory_id: u32, cell: Cell) -> Option<(usize, usize, usize, usize)> {
        let (ox, oy) = self.origin(inventory_id)?;
        let (w, h) = self.size(inventory_id)?;
        if cell.x >= w || cell.y >= h {
            return None;
        }
        let jump = self.geometry.jump;
        let side = (jump * (1.0 - 2.0 * INTERIOR_INSET)).floor().max(1.0) as usize;
        let left = (ox + (f64::from(cell.x) + INTERIOR_INSET) * jump).floor() - ox.floor();
        let top = (oy + (f64::from(cell.y) + INTERIOR_INSET) * jump).floor() - oy.floor();
        let (left, top) = (left.max(0.0) as usize, top.max(0.0) as usize);
        Some((left, top, left + side, top + side))
    }

    /// A resting point outside both grids: halfway between the bag's right edge and the stash's
    /// left edge, level with the middle of the bag.
    pub fn neutral_point(&self) -> (i32, i32) {
        let jump = self.geometry.jump;
        let (bag_w, bag_h) = self.size(self.bag_id).unwrap_or((0, 0));
        let bag_right = self.geometry.bag_origin.0 + f64::from(bag_w) * jump;
        let x = (bag_right + self.geometry.stash_origin.0) / 2.0;
        let y = self.geometry.bag_origin.1 + f64::from(bag_h) * jump / 2.0;
        (x.round() as i32, y.round() as i32)
    }

    /// Where the cursor rests while `step` is checked: the empty cell nearest to `near` that is at
    /// least two cells from both ends of the move, so neither an item tooltip nor a hover highlight
    /// reaches a probe. Falls back to one cell of distance, then to [`Self::neutral_point`].
    pub fn parking_point(&self, board: &Board, step: &RunStep, near: (f64, f64)) -> (i32, i32) {
        self.parking_point_avoiding(board, &footprints(step), near)
    }

    /// [`Self::parking_point`] keeping clear of any footprints, `(top-left, width, height)`.
    pub fn parking_point_avoiding(&self, board: &Board, avoid: &[(Location, u32, u32)], near: (f64, f64)) -> (i32, i32) {
        for distance in [PARK_DISTANCE, 1] {
            if let Some(found) = self.nearest_free_cell(board, avoid, near, distance) {
                return found;
            }
        }
        self.neutral_point()
    }

    fn nearest_free_cell(&self, board: &Board, avoid: &[(Location, u32, u32)], near: (f64, f64), distance: u32) -> Option<(i32, i32)> {
        let mut best: Option<(f64, (i32, i32))> = None;
        for inventory_id in [self.stash_id, self.bag_id] {
            let Some((w, h)) = self.size(inventory_id) else { continue };
            for y in 0..h {
                for x in 0..w {
                    let cell = Cell::new(x, y);
                    let clear = avoid.iter().all(|&(loc, fw, fh)| loc.inventory_id != inventory_id || cells_between(cell, loc.cell, fw, fh) >= distance);
                    if !clear || board.is_occupied(inventory_id, cell) {
                        continue;
                    }
                    let Some((cx, cy)) = self.center(Location::new(inventory_id, cell), 1, 1) else { continue };
                    let d2 = (cx - near.0).powi(2) + (cy - near.1).powi(2);
                    let closer = match best {
                        Some((best_d2, _)) => d2 < best_d2,
                        None => true,
                    };
                    if closer {
                        best = Some((d2, (cx.round() as i32, cy.round() as i32)));
                    }
                }
            }
        }
        best.map(|(_, at)| at)
    }
}

/// Both ends of a move as footprints: where the item starts and what it lands on.
pub fn footprints(step: &RunStep) -> [(Location, u32, u32); 2] {
    [(step.from, step.width, step.height), (step.to, step.to_width, step.to_height)]
}

/// Cells between `cell` and a `width`x`height` footprint at `origin` (0 when inside it), counting
/// diagonal steps as one.
fn cells_between(cell: Cell, origin: Cell, width: u32, height: u32) -> u32 {
    let gap = |c: u32, start: u32, len: u32| if c < start { start - c } else { c.saturating_sub(start + len.max(1) - 1) };
    gap(cell.x, origin.x, width).max(gap(cell.y, origin.y, height))
}

fn point(cell: Cell) -> Point {
    Point::new(cell.x as i32, cell.y as i32)
}
