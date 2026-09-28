//! Checking, before the first drag, that the stash on screen is the stash the plan was built from.
//! New in this port (Python dragged straight away): it catches the wrong tab being open (the locked
//! seasonal stash above all), stash data the game has not refreshed since the player last moved
//! things, and a grid that is not where the screen layout says.
//!
//! How: every cell's interior is captured. Cells the plan expects to be empty all show the same
//! empty-slot texture, so their per-pixel median is a template of "empty". Then most cells
//! expected empty must match that template, and most cells expected occupied must not. On the
//! wrong tab the expected pattern and the screen disagree, and one of the two shares drops.

use screen::Frame;

use super::verify::mean_abs_diff;

/// Mean absolute difference (per channel, 0-255) up to which a cell looks like the empty template.
const EMPTY_MATCH_MAX: f64 = 12.0;
/// Fewest expected-empty cells a template is built from.
const MIN_TEMPLATE_CELLS: usize = 3;
/// Most cells the template samples, spread evenly over the candidates.
const MAX_TEMPLATE_CELLS: usize = 48;
/// Share of expected-empty cells that must look empty.
const MIN_EMPTY_SHARE: f64 = 0.85;
/// Share of expected-occupied cells that must not look empty. Lower than the empty share: a corner
/// of a large item can be plain background.
const MIN_OCCUPIED_SHARE: f64 = 0.75;

/// One cell's interior as captured, and what the plan expects on it.
#[derive(Debug, Clone)]
pub struct CellPatch {
    /// In the stash (true) or the bag (false).
    pub in_stash: bool,
    pub expected_occupied: bool,
    pub pixels: Frame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutVerdict {
    /// The screen shows the expected stash and bag.
    Match,
    /// The screen shows something else.
    Mismatch,
    /// Too few expected-empty cells to learn what an empty cell looks like.
    Unreadable,
}

/// The outcome of [`check_layout`], with the counts behind it (stash and bag together).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutCheck {
    pub verdict: LayoutVerdict,
    /// Expected-empty cells that look empty, and how many were expected empty.
    pub empty_seen: usize,
    pub empty_total: usize,
    /// Expected-occupied cells that don't look empty, and how many were expected occupied.
    pub occupied_seen: usize,
    pub occupied_total: usize,
}

#[derive(Default, Clone, Copy)]
struct Tally {
    empty_seen: usize,
    empty_total: usize,
    occupied_seen: usize,
    occupied_total: usize,
}

impl Tally {
    fn passes(&self) -> bool {
        let share = |seen: usize, total: usize| seen as f64 / total as f64;
        (self.empty_total == 0 || share(self.empty_seen, self.empty_total) >= MIN_EMPTY_SHARE)
            && (self.occupied_total == 0 || share(self.occupied_seen, self.occupied_total) >= MIN_OCCUPIED_SHARE)
    }
}

/// Decides whether the captured cells show the expected layout. The stash and the bag must each
/// pass on their own, so a well-matched bag can't hide a wrong stash tab.
pub fn check_layout(patches: &[CellPatch]) -> LayoutCheck {
    let Some(template) = empty_template(patches) else {
        let expected_empty = patches.iter().filter(|p| !p.expected_occupied).count();
        return LayoutCheck {
            verdict: LayoutVerdict::Unreadable,
            empty_seen: 0,
            empty_total: expected_empty,
            occupied_seen: 0,
            occupied_total: patches.len() - expected_empty,
        };
    };
    let mut grids = [Tally::default(), Tally::default()];
    for patch in patches {
        let looks_empty = mean_abs_diff(&patch.pixels, &template).is_some_and(|diff| diff <= EMPTY_MATCH_MAX);
        let tally = &mut grids[usize::from(patch.in_stash)];
        if patch.expected_occupied {
            tally.occupied_total += 1;
            tally.occupied_seen += usize::from(!looks_empty);
        } else {
            tally.empty_total += 1;
            tally.empty_seen += usize::from(looks_empty);
        }
    }
    let verdict = if grids.iter().all(Tally::passes) { LayoutVerdict::Match } else { LayoutVerdict::Mismatch };
    LayoutCheck {
        verdict,
        empty_seen: grids[0].empty_seen + grids[1].empty_seen,
        empty_total: grids[0].empty_total + grids[1].empty_total,
        occupied_seen: grids[0].occupied_seen + grids[1].occupied_seen,
        occupied_total: grids[0].occupied_total + grids[1].occupied_total,
    }
}

/// The per-pixel median of expected-empty cells: the stash's when it has enough, else the bag's,
/// else both together.
fn empty_template(patches: &[CellPatch]) -> Option<Frame> {
    let empties = |in_stash: Option<bool>| -> Vec<&Frame> {
        patches
            .iter()
            .filter(|p| !p.expected_occupied && in_stash.is_none_or(|s| p.in_stash == s))
            .map(|p| &p.pixels)
            .collect()
    };
    let candidates = [empties(Some(true)), empties(Some(false)), empties(None)]
        .into_iter()
        .find(|frames| frames.len() >= MIN_TEMPLATE_CELLS)?;
    let step = candidates.len().div_ceil(MAX_TEMPLATE_CELLS).max(1);
    let first = candidates[0];
    let sample: Vec<&Frame> = candidates
        .iter()
        .step_by(step)
        .copied()
        .filter(|f| f.width() == first.width() && f.height() == first.height())
        .collect();
    if sample.len() < MIN_TEMPLATE_CELLS || first.data().is_empty() {
        return None;
    }
    let mut column = vec![0u8; sample.len()];
    let data = (0..first.data().len())
        .map(|i| {
            for (slot, frame) in column.iter_mut().zip(&sample) {
                *slot = frame.data()[i];
            }
            let mid = column.len() / 2;
            *column.select_nth_unstable(mid).1
        })
        .collect();
    Some(Frame::new(first.width(), first.height(), data))
}
