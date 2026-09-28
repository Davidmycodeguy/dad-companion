//! Shared test support: a deterministic PRNG and synthetic-stash generators, used by
//! `plan_invariants.rs` (and available to any other integration test that needs them). Not a test
//! file itself — included via `mod common;`.
#![allow(dead_code)] // not every test file uses every helper

use std::collections::HashMap;

use game_data::ItemCatalog;
use sorter::plan::{Cell, Grid};
use state::stash::STORAGE;
use state::OwnedItem;

/// A small, fixed item catalog covering every size the invariant tests place (1x1 up to 2x4), a
/// stackable potion, and five distinct 1x1 rarities (so a sort-order test has something to
/// distinguish without bin-packing getting in the way).
///
/// Field names must match `game_data::items::RawItem` *exactly* (`inventory_width`,
/// `inventory_height`, `max_stack_size`, snake_case — that struct has no `rename_all`, unlike
/// `OwnedItem` elsewhere in this codebase). Getting this wrong doesn't error: every unmatched
/// `#[serde(default)]` field just silently becomes its default, so every item would look 1x1 with
/// `max_stack` 1 — which will not overlap and will replay fine, quietly defeating the size- and
/// stacking-related invariants this file exists to check. Ask this module to build a stash and
/// spot-check a returned item's `width`/`max_stack` if these tests ever stop looking like they're
/// exercising anything.
pub fn test_catalog() -> ItemCatalog {
    ItemCatalog::from_json(
        r#"{
            "Coin_1":       {"name": "Coin",       "rarity": "Poor",      "inventory_width": 1, "inventory_height": 1},
            "Potion_1":     {"name": "Potion",     "rarity": "Common",    "inventory_width": 1, "inventory_height": 1, "max_stack_size": 5},
            "Ring_1":       {"name": "Ring",       "rarity": "Uncommon",  "inventory_width": 1, "inventory_height": 1},
            "Gem_1":        {"name": "Gem",        "rarity": "Legendary", "inventory_width": 1, "inventory_height": 1},
            "Relic_1":      {"name": "Relic",      "rarity": "Artifact",  "inventory_width": 1, "inventory_height": 1},
            "Dagger_1":     {"name": "Dagger",     "rarity": "Common",    "inventory_width": 1, "inventory_height": 2},
            "Sword_1":      {"name": "Sword",      "rarity": "Rare",      "inventory_width": 1, "inventory_height": 3},
            "Shield_1":     {"name": "Shield",     "rarity": "Epic",      "inventory_width": 2, "inventory_height": 2},
            "Armor_1":      {"name": "Armor",      "rarity": "Legendary", "inventory_width": 2, "inventory_height": 3},
            "Greatsword_1": {"name": "Greatsword", "rarity": "Unique",    "inventory_width": 2, "inventory_height": 4}
        }"#,
    )
    .expect("hand-written test catalog is valid JSON")
}

/// Every non-stackable item id in [`test_catalog`], smallest first, paired with its (width,
/// height). Smaller sizes are repeated so a uniform pick over this array is weighted toward them,
/// same as a real stash (mostly small loot, occasionally something big) — and, more importantly for
/// these tests, because a "completely full" grid dominated by 2-wide items is a genuinely much
/// harder (sometimes even unsolvable-by-any-greedy-heuristic) packing/shuffling puzzle than a
/// realistic one, which would say more about the limits of a greedy algorithm — Python's placement
/// heuristic has exactly the same limits — than about a real bug.
pub const ITEM_SIZES: [(&str, u32, u32); 16] = [
    ("Ring_1", 1, 1),
    ("Ring_1", 1, 1),
    ("Ring_1", 1, 1),
    ("Ring_1", 1, 1),
    ("Ring_1", 1, 1),
    ("Dagger_1", 1, 2),
    ("Dagger_1", 1, 2),
    ("Dagger_1", 1, 2),
    ("Dagger_1", 1, 2),
    ("Sword_1", 1, 3),
    ("Sword_1", 1, 3),
    ("Sword_1", 1, 3),
    ("Shield_1", 2, 2),
    ("Shield_1", 2, 2),
    ("Armor_1", 2, 3),
    ("Greatsword_1", 2, 4),
];

/// A small, fast, fully deterministic PRNG (SplitMix64) — no external crate needed for test data
/// that must reproduce exactly the same way on every run.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `[low, high)`. Panics if `high <= low`, same as any other range with nothing in
    /// it — every call site here supplies a real range.
    pub fn range(&mut self, low: u32, high: u32) -> u32 {
        assert!(high > low, "empty range");
        low + (self.next_u64() % u64::from(high - low)) as u32
    }

    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        self.range(0, denominator) < numerator
    }

    pub fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.range(0, items.len() as u32) as usize]
    }
}

pub fn owned(unique_id: u64, item_id: &str, count: u32, slot_id: u32, inventory_id: u32) -> OwnedItem {
    OwnedItem {
        unique_id,
        item_id: item_id.to_string(),
        count,
        contents: 0,
        inventory_id,
        slot_id: Some(slot_id),
        base: Vec::new(),
        rolls: Vec::new(),
        loot_state: 0,
        tradable: true,
    }
}

/// Scatters random-sized items at random positions in a `width`x`height` stash until roughly
/// `fill_ratio` of its cells are covered (or 5000 placement attempts are exhausted, for a
/// `fill_ratio` no random scatter can realistically reach). A non-overlapping placement is
/// guaranteed by construction — the generator itself uses [`Grid::fits`] to reject any attempt that
/// would overlap — which matters here because the invariant tests must start from *valid* input,
/// not merely feed the planner garbage and hope it copes.
pub fn scatter_items(rng: &mut Rng, width: u32, height: u32, fill_ratio: f64, next_id: &mut u64) -> Vec<OwnedItem> {
    let mut grid = Grid::new(width, height);
    let mut items = Vec::new();
    let target_filled = (f64::from(width * height) * fill_ratio) as u32;
    let mut filled = 0u32;
    let mut attempts = 0;
    while filled < target_filled && attempts < 5000 {
        attempts += 1;
        let &(item_id, w, h) = rng.choice(&ITEM_SIZES);
        if w > width || h > height {
            continue;
        }
        let cell = Cell::new(rng.range(0, width - w + 1), rng.range(0, height - h + 1));
        if !grid.fits(cell, w, h, None) {
            continue;
        }
        let id = *next_id;
        *next_id += 1;
        grid.place(id, cell, w, h);
        items.push(owned(id, item_id, 1, cell.y * width + cell.x, STORAGE));
        filled += w * h;
    }
    items
}

/// Fills a `width`x`height` stash completely (every cell covered, zero free), scanning
/// top-to-bottom/left-to-right and dropping a randomly-sized item on the first uncovered cell each
/// time (falling back to the guaranteed-to-fit 1x1 `Ring_1` at the grid's ragged edges). Used for
/// the "stash needs the workspace" case: with the stash fully occupied, the planner can only make
/// room for anything at all by buffering into the bag.
pub fn fill_completely(rng: &mut Rng, width: u32, height: u32, next_id: &mut u64) -> Vec<OwnedItem> {
    let mut grid = Grid::new(width, height);
    let mut items = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let cell = Cell::new(x, y);
            if !grid.is_free(cell) {
                continue;
            }
            let &(candidate_id, cw, ch) = rng.choice(&ITEM_SIZES);
            let (item_id, w, h) = if grid.fits(cell, cw, ch, None) { (candidate_id, cw, ch) } else { ("Ring_1", 1, 1) };
            let id = *next_id;
            *next_id += 1;
            grid.place(id, cell, w, h);
            items.push(owned(id, item_id, 1, cell.y * width + cell.x, STORAGE));
        }
    }
    items
}

/// `stack_count` separate 1x1 stacks of the same stackable item (`Potion_1`, max stack size 5),
/// each with a random quantity below the max, scattered non-overlapping across the stash — the
/// scenario `StackingEngine` exists to consolidate.
pub fn stackable_stash(rng: &mut Rng, width: u32, height: u32, stack_count: u32, next_id: &mut u64) -> Vec<OwnedItem> {
    let mut grid = Grid::new(width, height);
    let mut items = Vec::new();
    let mut placed = 0;
    let mut attempts = 0;
    while placed < stack_count && attempts < 2000 {
        attempts += 1;
        let cell = Cell::new(rng.range(0, width), rng.range(0, height));
        if !grid.fits(cell, 1, 1, None) {
            continue;
        }
        let id = *next_id;
        *next_id += 1;
        grid.place(id, cell, 1, 1);
        items.push(owned(id, "Potion_1", rng.range(1, 4), cell.y * width + cell.x, STORAGE));
        placed += 1;
    }
    items
}

/// A `Character` owning exactly `stash_items` (in `STORAGE`) and `bag_items` (in `BAG`).
pub fn character_with(stash_items: Vec<OwnedItem>, bag_items: Vec<OwnedItem>) -> state::Character {
    let mut items = stash_items;
    items.extend(bag_items);
    state::Character { items, ..state::Character::default() }
}

/// Replays `moves` on a fresh copy of `initial_stash`/`initial_bag`, asserting every move both
/// starts from where its item actually is and lands on a genuinely free cell — so an overlap or a
/// mis-recorded `from` fails at the exact move that caused it, not as a confusing mismatch at the
/// end. Returns the resulting stash grid for the caller to compare against the plan's positions.
pub fn replay_moves(
    initial_stash: &Grid,
    initial_bag: &Grid,
    stash_id: u32,
    dims: &HashMap<u64, (u32, u32)>,
    moves: &[sorter::plan::Move],
) -> Grid {
    let mut stash = initial_stash.clone();
    let mut bag = initial_bag.clone();
    for mv in moves {
        match *mv {
            sorter::plan::Move::Relocate { unique_id, from, to } => {
                let (w, h) = dims[&unique_id];
                take_from(&mut stash, &mut bag, stash_id, from, unique_id, w, h);
                place_at(&mut stash, &mut bag, stash_id, to, unique_id, w, h);
            }
            sorter::plan::Move::StackInto { unique_id, from, target_id, to } => {
                let (w, h) = dims[&unique_id];
                take_from(&mut stash, &mut bag, stash_id, from, unique_id, w, h);
                // The target already occupies `to` — merging just removes the source, it never
                // re-places anything there.
                let grid = if to.inventory_id == stash_id { &stash } else { &bag };
                assert_eq!(grid.occupant(to.cell), Some(target_id), "merge target {target_id} was not where the move said");
            }
        }
    }
    stash
}

fn take_from(stash: &mut Grid, bag: &mut Grid, stash_id: u32, location: sorter::plan::Location, id: u64, w: u32, h: u32) {
    let grid = if location.inventory_id == stash_id { &mut *stash } else { &mut *bag };
    assert_eq!(grid.occupant(location.cell), Some(id), "move's `from` did not match where item {id} actually was");
    grid.remove(location.cell, w, h);
}

fn place_at(stash: &mut Grid, bag: &mut Grid, stash_id: u32, location: sorter::plan::Location, id: u64, w: u32, h: u32) {
    let grid = if location.inventory_id == stash_id { &mut *stash } else { &mut *bag };
    assert!(grid.fits(location.cell, w, h, None), "move placed item {id} onto an already-occupied cell");
    grid.place(id, location.cell, w, h);
}
