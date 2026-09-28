//! A pure occupancy grid for one stash. Port of the grid parts of `storage.py`'s `Storage` (the
//! packet-parsing and mouse-macro parts stay out of planning).

use std::collections::BTreeSet;

use super::geometry::Cell;

/// One stash's occupancy, addressed by the unique id of whatever occupies each cell.
///
/// Python's `Storage.grid` stores `Item` objects (or `0` for empty) indexed `[x][y]`; this stores
/// `OwnedItem::unique_id`s indexed row-major, which is enough to answer every question the planner
/// asks and avoids needing item borrows inside the grid itself.
#[derive(Debug, Clone)]
pub struct Grid {
    width: u32,
    height: u32,
    cells: Vec<Option<u64>>,
    /// Cells held for an item that is logically "on hold" (e.g. buffered in the bag) so another
    /// placement doesn't reuse them first. Python: `Storage._reserved_slots`.
    reserved: BTreeSet<(u32, u32)>,
}

impl Grid {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, cells: vec![None; (width as usize) * (height as usize)], reserved: BTreeSet::new() }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn index(&self, cell: Cell) -> Option<usize> {
        if cell.x < self.width && cell.y < self.height {
            Some((cell.y * self.width + cell.x) as usize)
        } else {
            None
        }
    }

    /// Every cell an item rooted at `origin` with size `width`x`height` would occupy.
    pub fn footprint(origin: Cell, width: u32, height: u32) -> impl Iterator<Item = Cell> {
        (0..height).flat_map(move |dy| (0..width).map(move |dx| origin.offset(dx, dy)))
    }

    /// Whether the whole footprint lies on the grid.
    pub fn in_bounds(&self, origin: Cell, width: u32, height: u32) -> bool {
        origin.x + width <= self.width && origin.y + height <= self.height
    }

    pub fn occupant(&self, cell: Cell) -> Option<u64> {
        self.index(cell).and_then(|i| self.cells[i])
    }

    fn is_reserved(&self, cell: Cell) -> bool {
        self.reserved.contains(&(cell.x, cell.y))
    }

    /// A cell is free when it is on the grid, unoccupied, and not reserved.
    pub fn is_free(&self, cell: Cell) -> bool {
        self.index(cell).is_some() && self.occupant(cell).is_none() && !self.is_reserved(cell)
    }

    /// Whether an item of this size could sit at `origin` without overlapping anything else.
    /// `ignoring` lets an item check against the cells it already occupies itself.
    pub fn fits(&self, origin: Cell, width: u32, height: u32, ignoring: Option<u64>) -> bool {
        if !self.in_bounds(origin, width, height) {
            return false;
        }
        Self::footprint(origin, width, height).all(|cell| {
            self.is_free(cell) || (ignoring.is_some() && self.occupant(cell) == ignoring)
        })
    }

    /// Occupies `origin`'s footprint with `unique_id`. Invariant: the caller has already checked
    /// `fits` (or is intentionally overwriting, e.g. restoring a snapshot); out-of-bounds cells are
    /// silently skipped rather than panicking, matching Python's `Storage.move` which only ever
    /// writes cells it has already bounds-checked.
    pub fn place(&mut self, unique_id: u64, origin: Cell, width: u32, height: u32) {
        for cell in Self::footprint(origin, width, height) {
            if let Some(i) = self.index(cell) {
                self.cells[i] = Some(unique_id);
            }
        }
    }

    /// Clears `origin`'s footprint, regardless of what currently occupies it.
    pub fn remove(&mut self, origin: Cell, width: u32, height: u32) {
        for cell in Self::footprint(origin, width, height) {
            if let Some(i) = self.index(cell) {
                self.cells[i] = None;
            }
        }
    }

    /// Holds `origin`'s footprint so `find_empty_slot`/`is_free` treat it as unavailable even
    /// though it is not occupied. Python: adding to `Storage._reserved_slots`.
    pub fn reserve(&mut self, origin: Cell, width: u32, height: u32) {
        for cell in Self::footprint(origin, width, height) {
            self.reserved.insert((cell.x, cell.y));
        }
    }

    /// Releases a footprint reserved by `reserve`.
    pub fn release(&mut self, origin: Cell, width: u32, height: u32) {
        for cell in Self::footprint(origin, width, height) {
            self.reserved.remove(&(cell.x, cell.y));
        }
    }

    pub fn count_free_cells(&self) -> u32 {
        self.cells.iter().filter(|c| c.is_none()).count() as u32
    }

    /// Every occupied cell and its occupant, for building a `GridSnapshot` or iterating "every
    /// item currently on this grid".
    pub fn occupied_cells(&self) -> impl Iterator<Item = (Cell, u64)> + '_ {
        self.cells.iter().enumerate().filter_map(move |(i, occupant)| {
            occupant.map(|id| (Cell::new(i as u32 % self.width, i as u32 / self.width), id))
        })
    }

    /// The first `width`x`height` empty (and unreserved) slot, scanning bottom-to-top,
    /// right-to-left. Port of `Storage.find_empty_slot`: parking a blocker or buffering an item
    /// prefers the far corner of the grid, away from the top-left area the layout planner fills
    /// first, so temporary placements don't fight the final layout for space.
    pub fn find_empty_slot(&self, width: u32, height: u32) -> Option<Cell> {
        if width > self.width || height > self.height {
            return None;
        }
        let max_x = self.width - width;
        let max_y = self.height - height;
        for y in (0..=max_y).rev() {
            for x in (0..=max_x).rev() {
                let origin = Cell::new(x, y);
                if Self::footprint(origin, width, height).all(|cell| self.is_free(cell)) {
                    return Some(origin);
                }
            }
        }
        None
    }
}

/// A snapshot of a grid's occupancy, for later comparison against the live grid. Port of
/// `sort.py`'s `GridSnapshot`, used there to detect drift after a failed or mis-applied move; here
/// it also backs the invariant tests that replay a plan's moves and check the result matches the
/// target layout exactly.
#[derive(Debug, Clone)]
pub struct GridSnapshot {
    width: u32,
    height: u32,
    cells: Vec<Option<u64>>,
}

impl GridSnapshot {
    pub fn capture(grid: &Grid) -> Self {
        Self { width: grid.width, height: grid.height, cells: grid.cells.clone() }
    }

    /// `(cell, expected, actual)` for every cell whose occupant differs from the snapshot.
    pub fn diff(&self, grid: &Grid) -> Vec<(Cell, Option<u64>, Option<u64>)> {
        let width = self.width.min(grid.width);
        let height = self.height.min(grid.height);
        let mut diffs = Vec::new();
        for cell in Grid::footprint(Cell::new(0, 0), width, height) {
            let expected = self.cells[(cell.y * self.width + cell.x) as usize];
            let actual = grid.occupant(cell);
            if expected != actual {
                diffs.push((cell, expected, actual));
            }
        }
        diffs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grid_is_entirely_free() {
        let grid = Grid::new(3, 2);
        assert!(grid.is_free(Cell::new(0, 0)));
        assert!(grid.is_free(Cell::new(2, 1)));
        assert!(!grid.is_free(Cell::new(3, 0))); // out of bounds
    }

    #[test]
    fn fits_respects_bounds_and_ignoring() {
        let grid = Grid::new(2, 2);
        assert!(grid.fits(Cell::new(0, 0), 2, 2, None));
        assert!(!grid.fits(Cell::new(1, 0), 2, 2, None)); // runs off the right edge
    }
}
