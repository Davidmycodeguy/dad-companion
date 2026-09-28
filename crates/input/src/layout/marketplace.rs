//! The Marketplace ("My Listings" / "View Market") and merchant screens. Ports
//! `marketplace_layout.py` in full.

use std::collections::HashMap;

use crate::round::python_round;

pub const SPOTS_PER_PAGE: i32 = 10;
pub const INVENTORY_COLUMNS: i32 = 10;
pub const INVENTORY_ROWS: i32 = 5;
pub const STASH_COLUMNS: i32 = 12;
pub const STASH_ROWS: i32 = 20;
pub const MERCHANT_CARD_COLUMNS: i32 = 7;
const FIRST_STASH_TAB_ID: i32 = 4;
const GEAR_SET_FIRST_ID: i32 = 100;

/// Mirrors `market_rules.INVENTORY_STASH_ID`, also ported (independently) as
/// `market::rules::INVENTORY_STASH_ID` in the `market` crate. Duplicated rather than adding a
/// dependency on `market` purely for one string constant — see this crate's final report — so
/// keep the two in sync if either ever changes.
pub const INVENTORY_STASH_ID: &str = "2";

/// Measured from 16:9 screenshots, expressed at 1920x1080. Ports `BASE_POINTS`.
const BASE_POINTS: &[(&str, f64, f64)] = &[
    ("spot_row_origin", 298.0, 516.0),
    ("next_page_arrow", 374.0, 1025.0),
    ("prev_page_arrow", 219.0, 1026.0),
    ("tab_icon_origin", 1315.0, 196.0),
    ("inv_grid_origin", 1443.0, 622.0),
    ("stash_grid_origin", 1369.0, 184.0),
    ("price_field", 960.0, 618.0),
    ("create_listing_button", 960.0, 968.0),
    ("form_search_button", 960.0, 467.0),
    ("quantity_field", 960.0, 402.0),
    ("market_attr_reset", 1677.0, 207.0),
    ("market_search_button", 1794.0, 277.0),
    ("my_listings_tab", 1062.0, 123.0),
    ("market_next_page", 1032.0, 1015.0),
    ("confirm_listing_yes", 861.0, 620.0),
    ("confirm_listing_no", 1057.0, 620.0),
    ("transfer_all_button", 960.0, 657.0),
    ("view_market_tab", 862.0, 123.0),
    ("market_reset_filters", 1794.0, 207.0),
    ("rarity_dropdown", 375.0, 207.0),
    ("rarity_option_first", 298.0, 281.0),
    ("class_dropdown", 612.0, 207.0),
    ("class_option_first", 535.0, 281.0),
    ("trade_tab", 1200.0, 42.0),
    ("marketplace_button", 1190.0, 252.0),
    ("merchants_tab", 1041.0, 42.0),
    ("merchant_card_origin", 271.5, 330.0),
    ("merchant_sell_tab", 331.5, 204.0),
    ("merchant_sell_mode", 906.0, 415.0),
    ("merchant_make_deal", 958.5, 1014.0),
    ("sell_box_origin", 735.0, 442.5),
];

/// Ports `BASE_LENGTHS`.
const BASE_LENGTHS: &[(&str, f64)] = &[
    ("spot_row_spacing", 50.0),
    ("tab_icon_spacing", 46.5),
    ("cell", 41.3),
    ("rarity_option_spacing", 24.9),
    ("next_page_step", 17.0),
    ("merchant_card_dx", 229.0),
    ("merchant_card_dy", 286.5),
    ("sell_cell", 45.0),
];

/// `order_index`'s (row, column) on a 10-spots-per-page listings screen. Ports `spot_location`.
pub fn spot_location(order_index: i32) -> (i32, i32) {
    (order_index.div_euclid(SPOTS_PER_PAGE), order_index.rem_euclid(SPOTS_PER_PAGE))
}

/// Marketplace tab icons follow the account's stash ids in ascending order. Ports `auto_tab_order`.
pub fn auto_tab_order(stash_ids: &[&str]) -> Vec<i32> {
    let mut ids: Vec<i32> = stash_ids
        .iter()
        .filter_map(|raw| raw.parse::<i32>().ok())
        .filter(|value| (FIRST_STASH_TAB_ID..GEAR_SET_FIRST_ID).contains(value))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The tab icon index for `stash_id` under `tab_mapping` (inventory is always icon 0), or `None`
/// if it isn't a mapped stash tab. Ports `tab_icon_index`.
pub fn tab_icon_index(stash_id: &str, tab_mapping: &[i32]) -> Option<usize> {
    if stash_id == INVENTORY_STASH_ID {
        return Some(0);
    }
    let stash_type: i32 = stash_id.parse().ok()?;
    if stash_type == 0 {
        return None;
    }
    tab_mapping.iter().position(|&v| v == stash_type).map(|idx| 1 + idx)
}

/// Screen coordinates for the Marketplace/merchant screens at one resolution (and, optionally,
/// one windowed-mode offset and calibration). Ports `MarketplaceLayout`.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketplaceLayout {
    points: HashMap<&'static str, (i32, i32)>,
    lengths: HashMap<&'static str, f64>,
}

impl MarketplaceLayout {
    /// The raw point for a `BASE_POINTS` key. Panics on an unknown key, matching the Python
    /// reference's plain `dict[key]` lookup.
    pub fn point(&self, key: &str) -> (i32, i32) {
        self.points[key]
    }

    fn offset(&self, key: &str, dx: f64, dy: f64) -> (i32, i32) {
        let (x, y) = self.points[key];
        (python_round(x as f64 + dx) as i32, python_round(y as f64 + dy) as i32)
    }

    /// The raw (already scaled + calibrated) length for a `BASE_LENGTHS` key. Panics on an
    /// unknown key, matching the Python reference's plain `dict[key]` lookup.
    pub fn length(&self, key: &str) -> f64 {
        self.lengths[key]
    }

    pub fn spot_row(&self, row: i32) -> (i32, i32) {
        self.offset("spot_row_origin", 0.0, self.lengths["spot_row_spacing"] * row as f64)
    }

    /// Checkbox for rarity 1 (Poor) .. 8 (Artifact) in the open Rarity dropdown.
    pub fn rarity_option(&self, rarity: i32) -> (i32, i32) {
        self.offset("rarity_option_first", 0.0, self.lengths["rarity_option_spacing"] * (rarity - 1) as f64)
    }

    /// Checkbox for class index 0 (Barbarian) .. 9 (Wizard) in the open Class dropdown.
    pub fn class_option(&self, index: i32) -> (i32, i32) {
        self.offset("class_option_first", 0.0, self.lengths["rarity_option_spacing"] * index as f64)
    }

    /// The next-page arrow shifts right as the page counter widens ("1 / 13" vs "1 / 5,704").
    pub fn next_page_candidate(&self, attempt: i32) -> (i32, i32) {
        self.offset("market_next_page", self.lengths["next_page_step"] * attempt as f64, 0.0)
    }

    pub fn tab_icon(&self, icon_index: i32) -> (i32, i32) {
        self.offset("tab_icon_origin", 0.0, self.lengths["tab_icon_spacing"] * icon_index as f64)
    }

    /// Centre of the `index`-th card (row by row) on the Merchants & Travelers grid.
    pub fn merchant_card(&self, index: i32) -> (i32, i32) {
        let (row, col) = (index.div_euclid(MERCHANT_CARD_COLUMNS), index.rem_euclid(MERCHANT_CARD_COLUMNS));
        self.offset("merchant_card_origin", self.lengths["merchant_card_dx"] * col as f64, self.lengths["merchant_card_dy"] * row as f64)
    }

    /// Centre of the cells an item of `width` x `height` takes in the Sell box from `(col, row)`.
    pub fn sell_box_centre(&self, col: i32, row: i32, width: i32, height: i32) -> (i32, i32) {
        let cell = self.lengths["sell_cell"];
        self.offset("sell_box_origin", cell * (col as f64 + width as f64 / 2.0), cell * (row as f64 + height as f64 / 2.0))
    }

    pub fn item_centre(&self, stash_id: &str, slot_id: i32, width: i32, height: i32) -> (i32, i32) {
        let is_inv = stash_id == INVENTORY_STASH_ID;
        let columns = if is_inv { INVENTORY_COLUMNS } else { STASH_COLUMNS };
        let origin = if is_inv { "inv_grid_origin" } else { "stash_grid_origin" };
        let (row, col) = (slot_id.div_euclid(columns), slot_id.rem_euclid(columns));
        let cell = self.lengths["cell"];
        self.offset(origin, cell * (col as f64 + width as f64 / 2.0), cell * (row as f64 + height as f64 / 2.0))
    }

    pub fn hover_targets(&self) -> Vec<(&'static str, (i32, i32))> {
        vec![
            ("spot row 1", self.spot_row(0)),
            ("spot row 10", self.spot_row(SPOTS_PER_PAGE - 1)),
            ("next page arrow", self.point("next_page_arrow")),
            ("inventory icon", self.tab_icon(0)),
            ("first stash tab icon", self.tab_icon(1)),
            ("inventory first cell", self.item_centre(INVENTORY_STASH_ID, 0, 1, 1)),
            ("inventory last cell", self.item_centre(INVENTORY_STASH_ID, INVENTORY_COLUMNS * INVENTORY_ROWS - 1, 1, 1)),
            ("stash first cell", self.item_centre("4", 0, 1, 1)),
            ("stash last cell", self.item_centre("4", STASH_COLUMNS * STASH_ROWS - 1, 1, 1)),
            ("price field", self.point("price_field")),
            ("create listing button", self.point("create_listing_button")),
        ]
    }
}

/// Numeric JSON value, rejecting bools (`serde_json` would otherwise happily read `true` as `1`).
/// Ports `marketplace_layout._number`.
fn number(value: &serde_json::Value) -> Option<f64> {
    if value.is_boolean() {
        return None;
    }
    value.as_f64()
}

/// A `[dx, dy]` calibration delta. Ports `marketplace_layout._point_delta`.
fn point_delta(value: &serde_json::Value) -> Option<(f64, f64)> {
    let pair = value.as_array().filter(|a| a.len() == 2)?;
    Some((number(&pair[0])?, number(&pair[1])?))
}

/// Builds the layout for `resolution`, offsetting every point by `window_origin` (the game
/// window's client-area top-left in windowed mode; `(0, 0)` for fullscreen/borderless) and then
/// applying `calibration`'s `points`/`lengths` deltas. Malformed calibration entries (wrong shape,
/// non-numeric) are silently ignored, exactly as the Python reference does. Ports `build_layout`.
pub fn build_layout(resolution: (u32, u32), window_origin: (i32, i32), calibration: &serde_json::Value) -> MarketplaceLayout {
    let scale = super::scale_for(resolution);
    let point_deltas = calibration.get("points").and_then(|v| v.as_object());
    let length_deltas = calibration.get("lengths").and_then(|v| v.as_object());
    let (ox, oy) = (window_origin.0 as f64, window_origin.1 as f64);

    let mut points = HashMap::with_capacity(BASE_POINTS.len());
    for &(key, bx, by) in BASE_POINTS {
        let (x, y) = super::scale_point(bx, by, scale);
        let (dx, dy) = point_deltas.and_then(|m| m.get(key)).and_then(point_delta).unwrap_or((0.0, 0.0));
        points.insert(key, (python_round(x as f64 + ox + dx) as i32, python_round(y as f64 + oy + dy) as i32));
    }

    let mut lengths = HashMap::with_capacity(BASE_LENGTHS.len());
    for &(key, base) in BASE_LENGTHS {
        let delta = length_deltas.and_then(|m| m.get(key)).and_then(number).unwrap_or(0.0);
        lengths.insert(key, (super::scale_length(base, scale) + delta).max(1.0));
    }

    MarketplaceLayout { points, lengths }
}
