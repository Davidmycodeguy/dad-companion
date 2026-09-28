//! 1:1 port of `DnDTools-src/UI/tests/test_marketplace_layout.py`.

use input::marketplace::{auto_tab_order, build_layout, spot_location, tab_icon_index, SPOTS_PER_PAGE};
use input::{scale_for, scale_length, scale_point, Scale};

const MAPPING: [i32; 8] = [4, 20, 5, 6, 7, 8, 9, 30];

fn no_calibration() -> serde_json::Value {
    serde_json::Value::Null
}

#[test]
fn scale_for_standard_and_ultrawide() {
    assert_eq!(scale_for((1920, 1080)), Scale { sx: 1.0, sy: 1.0, offset_x: 0.0 });
    assert_eq!(scale_for((3840, 2160)), Scale { sx: 2.0, sy: 2.0, offset_x: 0.0 });
    let uw = scale_for((3440, 1440));
    assert!((uw.sx - 1440.0 / 1080.0).abs() < 1e-9);
    assert!((uw.sy - 1440.0 / 1080.0).abs() < 1e-9);
    assert!((uw.offset_x - (3440.0 - 1440.0 * 16.0 / 9.0) / 2.0).abs() < 1e-9);
}

#[test]
fn scale_point_matches_existing_sorter_math() {
    // macros BASE_LAYOUT['stash'] = (1378, 199); 4K -> (2756, 398).
    assert_eq!(scale_point(1378.0, 199.0, scale_for((3840, 2160))), (2756, 398));
    assert!((scale_length(40.5, scale_for((2560, 1440))) - 54.0).abs() < 1e-9);
    assert_eq!(scale_length(0.1, scale_for((1280, 720))), 1.0);
}

#[test]
fn spot_location_pages_of_ten() {
    assert_eq!(SPOTS_PER_PAGE, 10);
    assert_eq!(spot_location(0), (0, 0));
    assert_eq!(spot_location(9), (0, 9));
    assert_eq!(spot_location(10), (1, 0));
    assert_eq!(spot_location(37), (3, 7));
}

#[test]
fn tab_icon_index_inventory_and_stash() {
    assert_eq!(tab_icon_index("2", &MAPPING), Some(0));
    assert_eq!(tab_icon_index("4", &MAPPING), Some(1));
    assert_eq!(tab_icon_index("5", &MAPPING), Some(3));
}

#[test]
fn tab_icon_index_unmapped_returns_none() {
    assert_eq!(tab_icon_index("9", &[4, 20, 5, 6, 7, 8, 0, 30]), None);
    assert_eq!(tab_icon_index("abc", &MAPPING), None);
}

#[test]
fn auto_tab_order_is_stash_ids_ascending() {
    // Verified in game: Marketplace tab icons follow the account's stash ids in ascending order;
    // inventory (2) and equipment (3) are not stash tabs.
    assert_eq!(auto_tab_order(&["2", "3", "4", "20", "5", "21", "30"]), vec![4, 5, 20, 21, 30]);
    assert_eq!(auto_tab_order(&["2", "3", "abc", "101"]), Vec::<i32>::new());
}

#[test]
fn build_layout_1080p_base_points() {
    let layout = build_layout((1920, 1080), (0, 0), &no_calibration());
    assert_eq!(layout.point("price_field"), (960, 618));
    assert_eq!(layout.point("create_listing_button"), (960, 968));
    assert_eq!(layout.spot_row(0), (298, 516));
    assert_eq!(layout.spot_row(2), (298, 616));
    assert_eq!(layout.tab_icon(1), (1315, 242)); // 196 + 46.5 = 242.5 -> rounds half to even
}

#[test]
fn build_layout_4k_scales_everything() {
    let layout = build_layout((3840, 2160), (0, 0), &no_calibration());
    assert_eq!(layout.point("price_field"), (1920, 1236));
    assert_eq!(layout.spot_row(1), (596, 1132));
}

#[test]
fn item_centre_uses_grid_width_per_stash() {
    let layout = build_layout((1920, 1080), (0, 0), &no_calibration());
    // inventory 10 columns: slot 12 -> col 2, row 1; 1x1 item. 1443 + 41.3*2.5 = 1546.25 -> 1546;
    // 622 + 41.3*1.5 = 683.95 -> 684.
    assert_eq!(layout.item_centre("2", 12, 1, 1), (1546, 684));
    // stash 12 columns: slot 13 -> col 1, row 1; 2x2 item. 1369 + 41.3*2 = 1451.6 -> 1452;
    // 184 + 41.3*2 = 266.6 -> 267.
    assert_eq!(layout.item_centre("4", 13, 2, 2), (1452, 267));
}

#[test]
fn window_origin_and_calibration_offsets() {
    let calibration = serde_json::json!({"points": {"price_field": [5, -3]}, "lengths": {"cell": 1.5}});
    let layout = build_layout((1920, 1080), (100, 50), &calibration);
    assert_eq!(layout.point("price_field"), (1065, 665));
    assert!((layout.length("cell") - 42.8).abs() < 1e-9);
}

#[test]
fn calibration_ignores_junk() {
    let calibration = serde_json::json!({"points": {"price_field": "x", "nope": [1, 1]}, "lengths": {"cell": "big"}});
    let layout = build_layout((1920, 1080), (0, 0), &calibration);
    assert_eq!(layout.point("price_field"), (960, 618));
}

#[test]
fn hover_targets_cover_every_click_point() {
    let layout = build_layout((1920, 1080), (0, 0), &no_calibration());
    let names: Vec<&str> = layout.hover_targets().into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        vec![
            "spot row 1",
            "spot row 10",
            "next page arrow",
            "inventory icon",
            "first stash tab icon",
            "inventory first cell",
            "inventory last cell",
            "stash first cell",
            "stash last cell",
            "price field",
            "create listing button",
        ]
    );
}

#[test]
fn merchant_screen_points_match_the_4k_game() {
    // Measured in game at 3840x2160.
    let layout = build_layout((3840, 2160), (0, 0), &no_calibration());
    assert_eq!(layout.point("merchants_tab"), (2082, 84));
    assert_eq!(layout.point("merchant_sell_tab"), (663, 408));
    assert_eq!(layout.point("merchant_sell_mode"), (1812, 830)); // middle "Sell" (next to "Buyback")
    assert_eq!(layout.point("merchant_make_deal"), (1917, 2028));
    let (cx, cy) = layout.merchant_card(2); // The Collector: first row, third card
    assert!((cx - 1458).abs() <= 3);
    assert_eq!(cy, 660);
    assert_eq!(layout.merchant_card(7), (543, 1233)); // Weaponsmith: second row, first card
}

#[test]
fn sell_box_cells_at_4k() {
    // Sell box grid lines measured at x = 1470 + 90k, y = 885 + 90k (3840x2160).
    let layout = build_layout((3840, 2160), (0, 0), &no_calibration());
    assert_eq!(layout.sell_box_centre(0, 0, 1, 1), (1515, 930));
    assert_eq!(layout.sell_box_centre(9, 5, 1, 1), (2325, 1380));
    assert_eq!(layout.sell_box_centre(2, 3, 2, 2), (1740, 1245));
}

#[test]
fn merchant_points_scale_to_1080p() {
    let layout = build_layout((1920, 1080), (0, 0), &no_calibration());
    assert_eq!(layout.point("merchant_make_deal"), (958, 1014));
    assert_eq!(layout.sell_box_centre(0, 0, 1, 1), (758, 464));
}
