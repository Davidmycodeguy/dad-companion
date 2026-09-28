//! Grid coordinates and overlap testing. Port of `sort.py`'s free function `intersects` (the
//! `Point` type it uses is ported here as `Cell`).

/// A cell in a stash grid: `(x, y)`, zero-based from the top-left. Python: `Point`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
}

impl Cell {
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }

    /// The cell `dx` right and `dy` down from this one.
    pub fn offset(self, dx: u32, dy: u32) -> Self {
        Self { x: self.x + dx, y: self.y + dy }
    }
}

impl From<(u32, u32)> for Cell {
    fn from((x, y): (u32, u32)) -> Self {
        Self { x, y }
    }
}

/// Which stash a cell belongs to: the two are only ever the stash being sorted and the bag used
/// as scratch space, but moves name both explicitly so a replay never has to guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Location {
    pub inventory_id: u32,
    pub cell: Cell,
}

impl Location {
    pub const fn new(inventory_id: u32, cell: Cell) -> Self {
        Self { inventory_id, cell }
    }
}

/// Whether an `a_width`x`a_height` item at `a` overlaps a `b_width`x`b_height` item at `b`.
///
/// Port of `sort.py`'s `intersects`: two axis-aligned rectangles miss each other exactly when one
/// is entirely to the left of, right of, above, or below the other.
pub fn intersects(a: Cell, a_width: u32, a_height: u32, b: Cell, b_width: u32, b_height: u32) -> bool {
    if a.x + a_width <= b.x || b.x + b_width <= a.x {
        return false;
    }
    if a.y + a_height <= b.y || b.y + b_height <= a.y {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disjoint_rectangles_do_not_intersect() {
        assert!(!intersects(Cell::new(0, 0), 2, 2, Cell::new(2, 0), 2, 2));
        assert!(!intersects(Cell::new(0, 0), 2, 2, Cell::new(0, 2), 2, 2));
    }

    #[test]
    fn overlapping_rectangles_intersect() {
        assert!(intersects(Cell::new(0, 0), 2, 2, Cell::new(1, 1), 2, 2));
    }

    #[test]
    fn touching_edges_do_not_intersect() {
        // Shares the boundary line x=2 but occupies no common cell.
        assert!(!intersects(Cell::new(0, 0), 2, 2, Cell::new(2, 0), 1, 1));
    }
}
