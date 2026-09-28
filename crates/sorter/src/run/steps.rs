//! The plan as a list of drags, each checked against the stash it was built from before anything
//! runs. Python replayed its recorded `(start_stash, start_pos, end_stash, end_pos, ...)` tuples
//! against the rewound grid and fell back to a raw drag when the grid disagreed; here a plan that
//! disagrees with the stash is refused up front instead.

use std::collections::HashMap;

use game_data::ItemCatalog;
use state::{is_off_limits, Character};

use super::report::StopReason;
use crate::plan::{Cell, Grid, Location, Move, SortPlan, World};

/// What a drag does when it lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// The item moves to empty cells.
    Relocate,
    /// The item is dropped on another stack of the same item and merges into it.
    Stack { target_id: u64 },
}

/// One drag the run performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStep {
    pub unique_id: u64,
    pub item_id: String,
    pub name: String,
    pub kind: StepKind,
    pub from: Location,
    pub to: Location,
    /// The moving item's size in cells.
    pub width: u32,
    pub height: u32,
    /// Size of what the drop lands on: the item itself for a relocate, the target stack for a merge.
    pub to_width: u32,
    pub to_height: u32,
}

impl RunStep {
    /// Manhattan distance in cells between where the item starts and where it lands (across grids,
    /// as Python's move-outcome features measured it).
    pub fn distance_cells(&self) -> u32 {
        self.from.cell.x.abs_diff(self.to.cell.x) + self.from.cell.y.abs_diff(self.to.cell.y)
    }
}

/// Which cells of the stash and the bag are occupied, and by what: the sorter's picture of the
/// game, advanced one move at a time.
#[derive(Debug, Clone)]
pub struct Board {
    stash_id: u32,
    bag_id: u32,
    stash: Grid,
    bag: Grid,
    items: HashMap<u64, (Location, u32, u32)>,
}

impl Board {
    /// The stash and bag exactly as `world` loaded them.
    pub fn from_world(world: &World) -> Self {
        let items = world
            .items()
            .filter_map(|item| world.location(item.unique_id).map(|loc| (item.unique_id, (loc, item.width, item.height))))
            .collect();
        Board { stash_id: world.stash_id, bag_id: world.bag_id, stash: world.stash.clone(), bag: world.bag.clone(), items }
    }

    pub fn stash_id(&self) -> u32 {
        self.stash_id
    }

    pub fn bag_id(&self) -> u32 {
        self.bag_id
    }

    /// The stash or bag grid; `None` for any other inventory.
    pub fn grid(&self, inventory_id: u32) -> Option<&Grid> {
        if inventory_id == self.stash_id {
            Some(&self.stash)
        } else if inventory_id == self.bag_id {
            Some(&self.bag)
        } else {
            None
        }
    }

    fn grid_mut(&mut self, inventory_id: u32) -> Option<&mut Grid> {
        if inventory_id == self.stash_id {
            Some(&mut self.stash)
        } else if inventory_id == self.bag_id {
            Some(&mut self.bag)
        } else {
            None
        }
    }

    pub fn location(&self, unique_id: u64) -> Option<Location> {
        self.items.get(&unique_id).map(|(loc, _, _)| *loc)
    }

    /// Every item on `inventory_id`: `(unique_id, top-left cell, width, height)`.
    pub fn items_on(&self, inventory_id: u32) -> impl Iterator<Item = (u64, Cell, u32, u32)> + '_ {
        self.items
            .iter()
            .filter(move |(_, (loc, _, _))| loc.inventory_id == inventory_id)
            .map(|(&id, &(loc, width, height))| (id, loc.cell, width, height))
    }

    /// Whether anything sits on `cell` of `inventory_id`. Cells off the grid count as occupied.
    pub fn is_occupied(&self, inventory_id: u32, cell: Cell) -> bool {
        match self.grid(inventory_id) {
            Some(grid) => !grid.in_bounds(cell, 1, 1) || grid.occupant(cell).is_some(),
            None => true,
        }
    }

    /// Applies one step, refusing a step that does not start where the item is or lands on
    /// something it should not.
    pub fn apply(&mut self, step: &RunStep) -> Result<(), String> {
        let Some(&(at, width, height)) = self.items.get(&step.unique_id) else {
            return Err(format!("item {} is not in the stash or bag", step.unique_id));
        };
        if at != step.from {
            return Err(format!("item {} is not where the plan starts it", step.unique_id));
        }
        match step.kind {
            StepKind::Relocate => {
                let fits = self.grid(step.to.inventory_id).is_some_and(|g| g.fits(step.to.cell, width, height, Some(step.unique_id)));
                if !fits {
                    return Err(format!("item {} would land on another item", step.unique_id));
                }
                self.grid_mut(at.inventory_id).expect("item is on a known grid").remove(at.cell, width, height);
                self.grid_mut(step.to.inventory_id).expect("checked above").place(step.unique_id, step.to.cell, width, height);
                self.items.insert(step.unique_id, (step.to, width, height));
            }
            StepKind::Stack { target_id } => {
                if self.location(target_id) != Some(step.to) {
                    return Err(format!("the stack item {} should merge into is not at the drop spot", step.unique_id));
                }
                self.grid_mut(at.inventory_id).expect("item is on a known grid").remove(at.cell, width, height);
                self.items.remove(&step.unique_id);
            }
        }
        Ok(())
    }
}

/// The plan's moves as drags, validated against the stash and bag they start from.
#[derive(Debug, Clone)]
pub struct RunSteps {
    stash_id: u32,
    bag_id: u32,
    steps: Vec<RunStep>,
    initial: Board,
    finished: Board,
}

impl RunSteps {
    /// Turns `plan` (built for `character`'s stash `stash_id`) into drags. Refuses the locked
    /// seasonal stash, any move that starts or ends outside the stash being sorted and the bag,
    /// and any move that does not follow from the one before it.
    pub fn new(character: &Character, catalog: &ItemCatalog, stash_id: u32, plan: &SortPlan) -> Result<Self, StopReason> {
        if is_off_limits(stash_id) {
            return Err(StopReason::Refused("The locked seasonal stash is never sorted.".into()));
        }
        let world = World::build(character, catalog, stash_id).map_err(|err| StopReason::Refused(format!("This stash can't be sorted: {err}.")))?;
        let initial = Board::from_world(&world);
        let mut board = initial.clone();
        let mut steps = Vec::with_capacity(plan.moves.len());
        for planned in &plan.moves {
            let step = step_for(planned, &world)?;
            board.apply(&step).map_err(|why| StopReason::Refused(format!("The plan doesn't fit this stash ({why}); build it again.")))?;
            steps.push(step);
        }
        Ok(RunSteps { stash_id, bag_id: world.bag_id, steps, initial, finished: board })
    }

    pub fn stash_id(&self) -> u32 {
        self.stash_id
    }

    pub fn bag_id(&self) -> u32 {
        self.bag_id
    }

    pub fn steps(&self) -> &[RunStep] {
        &self.steps
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The stash and bag before the first move: what the screen must show when the run starts.
    pub fn initial(&self) -> &Board {
        &self.initial
    }

    /// The stash and bag after the last move.
    pub fn finished(&self) -> &Board {
        &self.finished
    }
}

fn step_for(planned: &Move, world: &World) -> Result<RunStep, StopReason> {
    let (unique_id, from, to, kind) = match *planned {
        Move::Relocate { unique_id, from, to } => (unique_id, from, to, StepKind::Relocate),
        Move::StackInto { unique_id, from, target_id, to } => (unique_id, from, to, StepKind::Stack { target_id }),
    };
    for inventory_id in [from.inventory_id, to.inventory_id] {
        if is_off_limits(inventory_id) {
            return Err(StopReason::Refused("Nothing is ever moved into or out of the locked seasonal stash.".into()));
        }
        if inventory_id != world.stash_id && inventory_id != world.bag_id {
            return Err(StopReason::Refused(format!("The plan touches stash {inventory_id}, which isn't being sorted.")));
        }
    }
    let item = world.item(unique_id).ok_or_else(|| StopReason::Refused(format!("The plan moves item {unique_id}, which isn't in this stash.")))?;
    let (to_width, to_height) = match kind {
        StepKind::Relocate => (item.width, item.height),
        StepKind::Stack { target_id } => {
            let target = world.item(target_id).ok_or_else(|| StopReason::Refused(format!("The plan merges into item {target_id}, which isn't in this stash.")))?;
            (target.width, target.height)
        }
    };
    Ok(RunStep {
        unique_id,
        item_id: item.item_id.clone(),
        name: item.name.clone(),
        kind,
        from,
        to,
        width: item.width,
        height: item.height,
        to_width,
        to_height,
    })
}
