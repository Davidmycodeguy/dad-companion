//! The simulated game. Pressing the button over an item picks it up, releasing drops it centred on
//! the cursor (merging into an equal stack, or going back where it came from when the spot is
//! taken), pressing while holding an item drops it (the drag's "reliability click"), and pressing a
//! tab selector opens that tab. The screen shows every item as one flat colour on dark empty cells.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use game_data::ItemCatalog;
use input::{Cancel, InputEvent, InputSink};
use screen::{Frame, Region};
use sorter::plan::{intersects, Cell};
use sorter::run::{Machine, StopReason};
use state::stash::BAG;
use state::{grid_size, slot_cell, Character};

use super::{GEOMETRY, TAB_POINT};

/// Colour of an empty cell, and of the screen outside the grids.
pub const EMPTY: [u8; 3] = [22, 20, 18];
pub const BACKGROUND: [u8; 3] = [70, 70, 70];
const STASH_SIZE: (u32, u32) = (12, 20);
const BAG_SIZE: (u32, u32) = (10, 5);

#[derive(Debug, Clone, PartialEq)]
pub struct FakeItem {
    pub unique_id: u64,
    pub item_id: String,
    pub inventory_id: u32,
    pub cell: Cell,
    pub width: u32,
    pub height: u32,
}

#[derive(Default)]
pub struct GameState {
    pub items: Vec<FakeItem>,
    pub open_tab: u32,
    pub running: bool,
    pub focused: bool,
    /// Where the fake mouse was last put, and how far the "player" has pushed it since.
    pub cursor: (i32, i32),
    pub drift: (i32, i32),
    pub held: Option<FakeItem>,
    /// Items that never move, and items that ignore the first press on them.
    pub stuck: HashSet<u64>,
    pub flaky: HashSet<u64>,
    /// Items the game takes back a moment after they land (a refused move).
    pub snap_back: HashSet<u64>,
    /// Each taken-back item as it was before its move, and captures left until it goes back.
    reverts: Vec<(FakeItem, usize)>,
    pub events: Vec<InputEvent>,
    /// Cursor position of every press.
    pub presses: Vec<(i32, i32)>,
    pub tab_points: Vec<((i32, i32), u32)>,
    pub moves: usize,
    pub drops: usize,
    /// After this many cursor moves, the "player" pushes the mouse by this much.
    pub drift_after_moves: Option<(usize, (i32, i32))>,
    pub lose_focus_after_drops: Option<usize>,
    pub cancel_after_drops: Option<(usize, Cancel)>,
}

impl GameState {
    pub fn actual_cursor(&self) -> (i32, i32) {
        (self.cursor.0 + self.drift.0, self.cursor.1 + self.drift.1)
    }

    fn set_cursor(&mut self, x: i32, y: i32) {
        self.cursor = (x, y);
        self.moves += 1;
        if let Some((after, drift)) = self.drift_after_moves {
            if self.moves >= after {
                self.drift = drift;
            }
        }
    }

    /// The inventory shown under a screen point, and the point in cell units within it.
    fn grid_at(&self, x: f64, y: f64) -> Option<(u32, f64, f64)> {
        let areas = [(GEOMETRY.stash_origin, STASH_SIZE, self.open_tab), (GEOMETRY.bag_origin, BAG_SIZE, BAG)];
        areas.into_iter().find_map(|((ox, oy), (w, h), inventory_id)| {
            let (fx, fy) = ((x - ox) / GEOMETRY.jump, (y - oy) / GEOMETRY.jump);
            (fx >= 0.0 && fy >= 0.0 && fx < f64::from(w) && fy < f64::from(h)).then_some((inventory_id, fx, fy))
        })
    }

    fn item_at(&self, inventory_id: u32, cell: Cell) -> Option<usize> {
        self.items.iter().position(|i| i.inventory_id == inventory_id && intersects(i.cell, i.width, i.height, cell, 1, 1))
    }

    fn press(&mut self) {
        let at = self.actual_cursor();
        self.presses.push(at);
        if let Some(&(_, tab)) = self.tab_points.iter().find(|(p, _)| (p.0 - at.0).abs() <= 3 && (p.1 - at.1).abs() <= 3) {
            self.open_tab = tab;
            return;
        }
        if let Some(held) = self.held.take() {
            self.drop_item(held);
            return;
        }
        let Some((inventory_id, fx, fy)) = self.grid_at(f64::from(at.0), f64::from(at.1)) else { return };
        let Some(index) = self.item_at(inventory_id, Cell::new(fx as u32, fy as u32)) else { return };
        let id = self.items[index].unique_id;
        if self.stuck.contains(&id) || self.flaky.remove(&id) {
            return;
        }
        self.held = Some(self.items.remove(index));
    }

    fn release(&mut self) {
        if let Some(held) = self.held.take() {
            self.drop_item(held);
        }
    }

    fn drop_item(&mut self, item: FakeItem) {
        self.drops += 1;
        if self.lose_focus_after_drops == Some(self.drops) {
            self.focused = false;
        }
        if let Some((after, cancel)) = &self.cancel_after_drops {
            if *after == self.drops {
                cancel.cancel();
            }
        }
        let at = self.actual_cursor();
        let placed = self.grid_at(f64::from(at.0), f64::from(at.1)).and_then(|(inventory_id, fx, fy)| {
            let x = (fx - f64::from(item.width) / 2.0).round();
            let y = (fy - f64::from(item.height) / 2.0).round();
            (x >= 0.0 && y >= 0.0).then(|| (inventory_id, Cell::new(x as u32, y as u32)))
        });
        let Some((inventory_id, cell)) = placed else {
            self.items.push(item); // dropped off the grid: back where it came from
            return;
        };
        let (w, h) = if inventory_id == BAG { BAG_SIZE } else { STASH_SIZE };
        let blockers: Vec<&FakeItem> = self
            .items
            .iter()
            .filter(|i| i.inventory_id == inventory_id && intersects(i.cell, i.width, i.height, cell, item.width, item.height))
            .collect();
        let merges = blockers.len() == 1 && blockers[0].item_id == item.item_id && blockers[0].cell == cell;
        if merges {
            return; // merged into the stack already there
        }
        if blockers.is_empty() && cell.x + item.width <= w && cell.y + item.height <= h {
            if self.snap_back.remove(&item.unique_id) {
                self.reverts.push((item.clone(), 2));
            }
            self.items.push(FakeItem { inventory_id, cell, ..item });
        } else {
            self.items.push(item);
        }
    }

    /// Counts one capture toward every pending take-back, moving back the ones that are due.
    fn advance_reverts(&mut self) {
        for (original, left) in std::mem::take(&mut self.reverts) {
            if left > 0 {
                self.reverts.push((original, left - 1));
            } else if let Some(item) = self.items.iter_mut().find(|i| i.unique_id == original.unique_id) {
                item.inventory_id = original.inventory_id;
                item.cell = original.cell;
            }
        }
    }

    fn render(&self, (left, top, right, bottom): Region) -> Frame {
        let (width, height) = ((right - left).max(0) as usize, (bottom - top).max(0) as usize);
        let mut data = Vec::with_capacity(width * height * 3);
        for y in top..bottom {
            for x in left..right {
                let pixel = match self.grid_at(f64::from(x) + 0.5, f64::from(y) + 0.5) {
                    Some((inventory_id, fx, fy)) => match self.item_at(inventory_id, Cell::new(fx as u32, fy as u32)) {
                        Some(index) => colour(self.items[index].unique_id),
                        None => EMPTY,
                    },
                    None => BACKGROUND,
                };
                data.extend_from_slice(&pixel);
            }
        }
        Frame::new(width, height, data)
    }
}

/// A distinct flat colour per item, never close to an empty cell.
fn colour(unique_id: u64) -> [u8; 3] {
    let channel = |mul: u64| (60 + (unique_id * mul) % 180) as u8;
    [channel(67), channel(131), channel(29)]
}

/// The game, shared by its fake mouse and its fake screen.
#[derive(Clone)]
pub struct FakeGame(Arc<Mutex<GameState>>);

impl FakeGame {
    /// The game with `character`'s stash tabs and bag loaded, `open_tab` showing, not yet in front.
    pub fn new(character: &Character, catalog: &ItemCatalog, open_tab: u32) -> Self {
        let items = character
            .items
            .iter()
            .filter_map(|owned| {
                let (grid_width, _) = grid_size(owned.inventory_id)?;
                let (x, y) = slot_cell(owned.slot_id?, grid_width);
                let (width, height) = catalog.get(&owned.item_id).map_or((1, 1), |c| (c.width.max(1), c.height.max(1)));
                Some(FakeItem { unique_id: owned.unique_id, item_id: owned.item_id.clone(), inventory_id: owned.inventory_id, cell: Cell::new(x, y), width, height })
            })
            .collect();
        let state = GameState { items, open_tab, running: true, tab_points: vec![(TAB_POINT, state::stash::STORAGE)], ..GameState::default() };
        FakeGame(Arc::new(Mutex::new(state)))
    }

    pub fn state(&self) -> MutexGuard<'_, GameState> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn sink(&self) -> FakeSink {
        FakeSink(self.clone())
    }

    pub fn machine(&self) -> FakeMachine {
        FakeMachine(self.clone())
    }

    /// Where every item on `inventory_id` sits right now.
    pub fn layout(&self, inventory_id: u32) -> HashMap<u64, Cell> {
        self.state().items.iter().filter(|i| i.inventory_id == inventory_id).map(|i| (i.unique_id, i.cell)).collect()
    }

    /// Mouse button presses and releases so far.
    pub fn button_counts(&self) -> (usize, usize) {
        let state = self.state();
        let downs = state.events.iter().filter(|e| matches!(e, InputEvent::MouseDown)).count();
        let ups = state.events.iter().filter(|e| matches!(e, InputEvent::MouseUp)).count();
        (downs, ups)
    }
}

/// The fake mouse: every call goes to the game.
pub struct FakeSink(FakeGame);

impl InputSink for FakeSink {
    fn move_to(&mut self, x: i32, y: i32) {
        let mut state = self.0.state();
        state.events.push(InputEvent::MoveTo { x, y });
        state.set_cursor(x, y);
    }

    fn mouse_down(&mut self) {
        let mut state = self.0.state();
        state.events.push(InputEvent::MouseDown);
        state.press();
    }

    fn mouse_up(&mut self) {
        let mut state = self.0.state();
        state.events.push(InputEvent::MouseUp);
        state.release();
    }

    fn key_event(&mut self, vk: u16, key_up: bool) {
        self.0.state().events.push(InputEvent::Key { vk, key_up });
    }

    fn cursor_position(&self) -> (i32, i32) {
        self.0.state().actual_cursor()
    }
}

/// The fake screen and window.
pub struct FakeMachine(FakeGame);

impl Machine for FakeMachine {
    fn grab(&self, region: Region) -> Result<Frame, String> {
        let mut state = self.0.state();
        state.advance_reverts();
        Ok(state.render(region))
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        Some(self.0.state().actual_cursor())
    }

    fn game_focused(&self) -> bool {
        self.0.state().focused
    }

    fn bring_game_forward(&self, sink: &mut dyn InputSink) -> Result<(), StopReason> {
        if !self.0.state().running {
            return Err(StopReason::GameNotFound);
        }
        sink.key_event(input::VK_MENU, false);
        sink.key_event(input::VK_MENU, true);
        self.0.state().focused = true;
        Ok(())
    }
}
