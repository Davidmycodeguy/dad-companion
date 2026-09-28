//! Port of DnDTools' `tests/test_tooltip_finder.py`, plus a check that grabbing cropped regions
//! (via a fake `grab`) finds the same tooltip as running directly on the whole frame.

use tooltip::finder::{
    find_near_cursor, find_tooltip, find_tooltips, measure, probe, probe_strips, rank, rule_rows, rule_still_there,
    MAX_PROBE_ROWS,
};
use tooltip::{Frame, Region};

const EPIC: [u8; 3] = [150, 60, 195];
const LEGENDARY: [u8; 3] = [230, 120, 20];
const POOR: [u8; 3] = [82, 82, 82];

fn frame(width: usize, height: usize) -> Frame {
    Frame::filled(width, height, [8, 8, 8])
}

/// Sets every `step`-th column's pixel to `rgb`, for rows `top..bottom` and columns `left..right`:
/// a test-only stand-in for numpy's `frame[top:bottom, left:right:step] = rgb` fixture slicing.
fn fill_strided(frame: &mut Frame, top: usize, bottom: usize, left: usize, right: usize, step: usize, rgb: [u8; 3]) {
    for y in top..bottom.min(frame.height()) {
        let mut x = left;
        while x < right.min(frame.width()) {
            frame.set_pixel(x, y, rgb);
            x += step;
        }
    }
}

/// A dark panel whose title is underlined by two thin rarity-coloured rules (as in game at 4K).
fn tooltip(frame: &mut Frame, left: usize, rule_y: usize, width: usize, color: [u8; 3]) {
    frame.fill_rect(left, rule_y - 105, left + width, rule_y + 600, [20, 14, 22]);
    for y in [rule_y, rule_y + 6] {
        frame.fill_rect(left + 70, y, left + width - 70, y + 2, color);
    }
    fill_strided(frame, rule_y - 70, rule_y - 30, left + 150, left + width - 150, 6, [170, 90, 210]);
}

/// A Poor tooltip as drawn at 4K: two dim grey lines, 2 px thick, the second 5 px below the first.
fn poor_tooltip(frame: &mut Frame, left: usize, rule_y: usize, width: usize) {
    frame.fill_rect(left, rule_y - 105, left + width, rule_y + 600, [20, 18, 18]);
    for y in [rule_y, rule_y + 5] {
        frame.fill_rect(left + 40, y, left + width - 40, y + 2, POOR);
    }
}

/// Hands out regions of a frame like a screen capture, remembering what was grabbed.
struct FakeScreen {
    frame: Frame,
    grabs: Vec<Region>,
}

impl FakeScreen {
    fn new(frame: Frame) -> Self {
        Self { frame, grabs: Vec::new() }
    }

    fn size(&self) -> (i32, i32) {
        (self.frame.width() as i32, self.frame.height() as i32)
    }

    fn grab(&mut self, region: Region) -> Frame {
        self.grabs.push(region);
        let (left, top, right, bottom) = region;
        self.frame.crop(left.max(0) as usize, top.max(0) as usize, right.max(0) as usize, bottom.max(0) as usize)
    }
}

#[test]
fn finds_the_tooltip_nearest_the_cursor() {
    let mut f = frame(1600, 1400);
    tooltip(&mut f, 400, 500, 730, EPIC);
    let found = find_tooltip(&f, (380, 420), 1.0).unwrap();
    assert_eq!((found.left, found.right, found.rule_y), (470, 1060, 500));
    assert_eq!(found.color, EPIC);
}

#[test]
fn ocr_region_covers_title_and_body_within_the_frame() {
    let mut f = frame(1600, 1400);
    tooltip(&mut f, 400, 500, 730, EPIC);
    let found = find_tooltip(&f, (380, 420), 1.0).unwrap();
    let (left, top, right, bottom) = found.ocr_region(1600, 1400, 1.0);
    assert!(left < 400 + 70 && right > 400 + 730 - 70);
    assert!(top <= 500 - 100 && bottom == 1400);
}

#[test]
fn nothing_without_a_long_rule() {
    let mut f = frame(1600, 1400);
    f.fill_rect(400, 500, 560, 502, EPIC);
    assert!(find_tooltip(&f, (400, 400), 1.0).is_none());
}

#[test]
fn white_rules_of_common_tooltips_count_but_white_text_does_not() {
    let mut f = frame(1600, 1400);
    f.fill_rect(300, 500, 1000, 501, [174, 173, 173]);
    f.fill_rect(480, 500, 490, 501, [60, 60, 60]);
    let found = find_tooltip(&f, (290, 450), 1.0).unwrap();
    assert_eq!((found.left, found.right, found.color), (300, 1000, [174, 173, 173]));

    let mut text = frame(1600, 1400);
    let mut start = 300;
    while start < 800 {
        text.fill_rect(start, 500, start + 8, 501, [230, 230, 230]);
        start += 12;
    }
    assert!(find_tooltip(&text, (290, 450), 1.0).is_none());
}

#[test]
fn two_tooltips_pick_the_one_by_the_cursor() {
    let mut f = frame(2400, 1400);
    tooltip(&mut f, 100, 400, 700, EPIC);
    tooltip(&mut f, 1400, 400, 700, LEGENDARY);
    let found = find_tooltip(&f, (1380, 350), 1.0).unwrap();
    assert_eq!(found.color, LEGENDARY);
}

#[test]
fn scale_shrinks_the_minimum_rule_length() {
    let mut f = frame(800, 700);
    f.fill_rect(100, 250, 300, 251, EPIC);
    assert!(find_tooltip(&f, (90, 200), 0.5).is_some());
    assert!(find_tooltip(&f, (90, 200), 1.0).is_none());
}

#[test]
fn a_rule_broken_into_pieces_still_counts_whole() {
    let mut f = frame(1600, 1400);
    let mut start = 300;
    while start < 1000 {
        f.fill_rect(start, 500, start + 100, 502, EPIC);
        start += 110;
    }
    let found = find_tooltip(&f, (290, 450), 1.0).unwrap();
    assert_eq!((found.left, found.right), (300, 1060));
}

#[test]
fn only_the_area_around_the_cursor_is_searched() {
    let mut f = frame(4000, 1400);
    tooltip(&mut f, 3000, 500, 730, EPIC);
    assert!(find_tooltip(&f, (100, 400), 1.0).is_none());
    assert!(find_tooltip(&f, (2900, 400), 1.0).is_some());
}

#[test]
fn screen_borders_wider_than_any_tooltip_are_ignored() {
    let mut f = frame(3000, 1400);
    f.fill_rect(0, 1, 3000, 3, [230, 190, 40]);
    assert!(find_tooltip(&f, (1500, 200), 1.0).is_none());
}

#[test]
fn rule_pieces_on_neighbouring_rows_merge_into_one_rule() {
    let mut f = frame(1600, 1400);
    f.fill_rect(600, 500, 1000, 501, EPIC);
    f.fill_rect(300, 501, 620, 502, EPIC);
    let found = find_tooltip(&f, (290, 450), 1.0).unwrap();
    assert_eq!((found.left, found.right, found.rule_y), (300, 1000, 500));
}

#[test]
fn coloured_text_bridged_across_letters_is_not_a_rule() {
    let mut f = frame(1600, 1400);
    let mut start = 300;
    while start < 900 {
        f.fill_rect(start, 500, start + 5, 501, [155, 99, 69]);
        start += 10;
    }
    assert!(find_tooltip(&f, (290, 450), 1.0).is_none());
}

#[test]
fn several_candidates_come_nearest_first() {
    let mut f = frame(2400, 1400);
    tooltip(&mut f, 100, 400, 700, EPIC);
    tooltip(&mut f, 1400, 400, 700, LEGENDARY);
    let boxes = find_tooltips(&f, (1380, 350), 1.0, 3);
    let colors: Vec<[u8; 3]> = boxes.iter().map(|b| b.color).collect();
    assert_eq!(colors, vec![LEGENDARY, EPIC]);
}

#[test]
fn a_tooltip_opening_upward_beats_a_nearer_ui_line() {
    // Items low on screen open their tooltip upward: the rule is far above the cursor, but the
    // tooltip's body reaches down beside the item.
    let mut f = frame(3000, 1600);
    tooltip(&mut f, 1600, 300, 700, EPIC);
    f.fill_rect(700, 1100, 1000, 1101, [174, 173, 173]); // a fixed grey UI line, closer to the cursor's row
    let boxes = find_tooltips(&f, (1560, 1150), 1.0, 3);
    assert_eq!(boxes[0].rule_y, 300);
}

#[test]
fn probe_strips_sit_beside_the_cursor_and_stay_on_screen() {
    let mut strips = probe_strips((2000, 1000), 1.0, (3840, 2160));
    assert_eq!(strips.len(), 2);
    strips.sort_by_key(|s| -s.0);
    let (right, left) = (strips[0], strips[1]);
    assert!(right.0 > 2000 && left.2 < 2000); // one each side of the cursor
    assert!(strips.iter().all(|s| s.1 >= 0 && s.3 <= 2160));
    assert!(probe_strips((5, 1000), 1.0, (3840, 2160))[0].0 > 5); // no strip off the left edge
}

#[test]
fn rule_rows_are_where_a_rule_crosses_the_whole_strip() {
    let mut strip = frame(64, 600);
    strip.fill_rect(0, 200, 64, 202, EPIC);
    strip.fill_rect(0, 206, 64, 208, EPIC); // the second line of the same rule
    strip.fill_rect(0, 400, 20, 402, EPIC); // a short stroke, not a rule
    assert_eq!(rule_rows(&strip, 1.0), vec![200]);
}

#[test]
fn finds_the_tooltip_by_the_cursor_grabbing_only_strips_and_bands() {
    let mut f = frame(3840, 2160);
    tooltip(&mut f, 2030, 700, 760, EPIC);
    let mut screen = FakeScreen::new(f);
    let size = screen.size();
    let mut grab = |region: Region| screen.grab(region);
    let boxes = find_near_cursor(&mut grab, (2000, 1300), 1.0, size, 3);
    let first: Vec<(i32, i32, i32, [u8; 3])> = boxes.iter().take(1).map(|b| (b.left, b.right, b.rule_y, b.color)).collect();
    assert_eq!(first, vec![(2100, 2720, 700, EPIC)]);
    assert_eq!(screen.grabs.len(), 2); // both strips in one grab, then the rule's band
    let grabbed: i64 = screen.grabs.iter().map(|r| i64::from(r.2 - r.0) * i64::from(r.3 - r.1)).sum();
    assert!((grabbed as f64) < 0.15 * 3840.0 * 2160.0); // a small part of the screen
}

#[test]
fn a_tooltip_opening_left_of_the_cursor_is_found_too() {
    let mut f = frame(3840, 2160);
    tooltip(&mut f, 2400, 900, 760, EPIC);
    let mut screen = FakeScreen::new(f);
    let size = screen.size();
    let mut grab = |region: Region| screen.grab(region);
    let boxes = find_near_cursor(&mut grab, (3190, 1000), 1.0, size, 3);
    assert!(!boxes.is_empty());
    assert_eq!(boxes[0].rule_y, 900);
    assert_eq!(boxes[0].right, 3090);
}

#[test]
fn no_rule_beside_the_cursor_means_nothing_found() {
    let mut screen = FakeScreen::new(frame(3840, 2160));
    let size = screen.size();
    let mut grab = |region: Region| screen.grab(region);
    assert!(find_near_cursor(&mut grab, (2000, 1300), 1.0, size, 3).is_empty());
}

#[test]
fn a_poor_tooltips_dim_double_rule_is_found() {
    let mut f = frame(3840, 2160);
    poor_tooltip(&mut f, 2030, 700, 760);
    let mut screen = FakeScreen::new(f);
    let size = screen.size();
    let mut grab = |region: Region| screen.grab(region);
    let boxes = find_near_cursor(&mut grab, (2000, 1300), 1.0, size, 3);
    assert!(!boxes.is_empty());
    assert_eq!(boxes[0].rule_y, 700);
    assert_eq!(boxes[0].color, POOR);
}

#[test]
fn a_single_dim_ui_line_is_not_a_rule() {
    let mut f = frame(3840, 2160);
    f.fill_rect(2070, 700, 2750, 702, POOR);
    let mut screen = FakeScreen::new(f);
    let size = screen.size();
    let mut grab = |region: Region| screen.grab(region);
    assert!(find_near_cursor(&mut grab, (2000, 1300), 1.0, size, 3).is_empty());
}

#[test]
fn rule_rows_see_a_dim_double_line_but_not_a_single_one() {
    let mut strip = frame(64, 600);
    strip.fill_rect(0, 200, 64, 202, POOR);
    strip.fill_rect(0, 205, 64, 207, POOR);
    strip.fill_rect(0, 400, 64, 402, POOR);
    assert_eq!(rule_rows(&strip, 1.0), vec![200]);
}

#[test]
fn rule_still_there_detects_the_rule_in_its_own_colour() {
    let mut f = frame(600, 4);
    f.fill_rect(0, 0, 600, 2, EPIC);
    assert!(rule_still_there(&f, EPIC));
    assert!(!rule_still_there(&frame(600, 4), EPIC)); // the background alone isn't the rule
}

/// `probe` + `measure`, grabbing only cropped regions of a screen, must find the same tooltip (same
/// geometry and colour) that `find_tooltips` finds by scanning the whole frame directly.
#[test]
fn probe_and_measure_via_a_cropping_grab_agree_with_find_tooltips_on_the_whole_frame() {
    let mut f = frame(3840, 2160);
    tooltip(&mut f, 2030, 700, 760, EPIC);
    let cursor = (2000, 1300);
    let scale = 1.0;

    let direct = find_tooltips(&f, cursor, scale, 3);
    assert_eq!(direct.len(), 1);

    let mut screen = FakeScreen::new(f);
    let size = screen.size();
    let mut grab = |region: Region| screen.grab(region);
    let mut via_probe = Vec::new();
    for (row, _) in probe(&mut grab, cursor, scale, size).into_iter().take(MAX_PROBE_ROWS) {
        via_probe.extend(measure(&mut grab, row, cursor, scale, size, 3));
    }
    let ranked = rank(via_probe, cursor, scale);

    assert_eq!(ranked.first().copied(), direct.first().copied());
}

/// Not a Python-side test (no timing test exists in `test_tooltip_finder.py`): a guard against a
/// gross performance regression. The real budget (well under 100 ms in release mode, on a full
/// 3840x2160 frame) is measured separately with `--release`; debug builds run under plain `cargo
/// test` can be many times slower, so this bound is generous on purpose.
#[test]
fn find_tooltip_on_a_4k_frame_is_fast() {
    let mut f = frame(3840, 2160);
    tooltip(&mut f, 2030, 700, 760, EPIC);
    let start = std::time::Instant::now();
    let found = find_tooltip(&f, (2000, 1300), 1.0);
    let elapsed = start.elapsed();
    eprintln!("find_tooltip on a 4K frame took {elapsed:?}");
    assert!(found.is_some());
    assert!(elapsed.as_millis() < 2000, "find_tooltip took {elapsed:?}");
}
