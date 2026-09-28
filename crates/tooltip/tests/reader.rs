//! The tooltip reader, ported with the Python app's tests (tests/test_hover_reader.py).

use std::cell::RefCell;
use std::rc::Rc;

use tooltip::finder::TooltipBox;
use tooltip::parser::ItemIndex;
use tooltip::reader::{TextReader, TooltipReader};
use tooltip::{Frame, OcrLine, Region};

const DARK: [u8; 3] = [8, 8, 8];
const GREEN: [u8; 3] = [76, 150, 3];
/// A fixed UI line: bright and colourless, like a Common tooltip's rule.
const GREY: [u8; 3] = [174, 173, 173];
const SCREEN: (i32, i32) = (3000, 2000);

fn index() -> ItemIndex {
    ItemIndex::from_triples([("OccultistRobe_3001", "Occultist Robe", "Uncommon")])
}

fn robe() -> Vec<OcrLine> {
    vec![
        OcrLine::new("Occultist Robe", 98, 19, 162, 18),
        OcrLine::new("- +2% Undead Damage Bonus", 45, 214, 246, 16),
        OcrLine::new("Rarity: Uncommon", 101, 387, 150, 15),
    ]
}

fn merchant() -> Vec<OcrLine> {
    vec![OcrLine::new("The Collector", 60, 20, 180, 22), OcrLine::new("Rarity", 160, 800, 60, 16)]
}

/// Answers each read with the next canned result (nothing once they run out), counting reads.
#[derive(Clone)]
struct FakeOcr(Rc<RefCell<(Vec<Vec<OcrLine>>, usize)>>);

impl FakeOcr {
    fn new(answers: Vec<Vec<OcrLine>>) -> Self {
        Self(Rc::new(RefCell::new((answers, 0))))
    }
    fn reads(&self) -> usize {
        self.0.borrow().1
    }
}

impl TextReader for FakeOcr {
    fn read(&mut self, _: &Frame) -> Result<Vec<OcrLine>, String> {
        let mut inner = self.0.borrow_mut();
        inner.1 += 1;
        Ok(if inner.0.is_empty() { Vec::new() } else { inner.0.remove(0) })
    }
}

/// A screen that hands out regions of one frame and remembers what was grabbed.
struct FakeScreen {
    frame: Frame,
    grabs: Vec<Region>,
}

impl FakeScreen {
    fn new(frame: Frame) -> Self {
        Self { frame, grabs: Vec::new() }
    }

    fn grab(&mut self) -> impl FnMut(Region) -> Frame + '_ {
        |region: Region| {
            self.grabs.push(region);
            let (l, t, r, b) = region;
            let c = |v: i32, max: usize| (v.max(0) as usize).min(max);
            self.frame.crop(c(l, self.frame.width()), c(t, self.frame.height()), c(r, self.frame.width()), c(b, self.frame.height()))
        }
    }
}

fn screen_with_rule(left: usize, rule_y: usize, color: [u8; 3]) -> Frame {
    let mut frame = Frame::filled(3000, 2000, DARK);
    frame.fill_rect(left, rule_y, left + 600, rule_y + 2, color);
    frame
}

fn frame_with_rules(rules: &[(usize, usize, [u8; 3])]) -> Frame {
    let mut frame = Frame::filled(2000, 1600, DARK);
    for &(left, rule_y, color) in rules {
        frame.fill_rect(left, rule_y, left + 600, rule_y + 2, color);
    }
    frame
}

fn read_screen(reader: &mut TooltipReader<FakeOcr>, screen: &mut FakeScreen, cursor: (i32, i32)) -> Option<tooltip::reader::Found> {
    reader.read_screen(&mut screen.grab(), cursor, 1.0, SCREEN, 0.0).unwrap()
}

#[test]
fn reads_the_first_candidate_that_is_a_real_tooltip() {
    let frame = frame_with_rules(&[(300, 400, [140, 140, 140]), (900, 700, GREEN)]);
    let ocr = FakeOcr::new(vec![merchant(), robe()]);
    let found = TooltipReader::new(ocr, index()).read(&frame, (880, 650), 1.0).unwrap().unwrap();
    assert_eq!(found.tooltip.item_id, "OccultistRobe_3001");
    assert_eq!(found.tooltip.rolls, [("UndeadDamageMod".to_owned(), 20)]);
    assert!(found.tooltip_box.left >= 900);
}

#[test]
fn a_screen_already_read_is_not_read_again() {
    let frame = frame_with_rules(&[(900, 700, GREEN)]);
    let ocr = FakeOcr::new(vec![robe()]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    let first = reader.read(&frame, (880, 650), 1.0).unwrap();
    let again = reader.read(&frame, (880, 650), 1.0).unwrap();
    assert_eq!(first, again);
    assert_eq!(ocr.reads(), 1);
}

#[test]
fn lines_that_are_not_tooltips_are_remembered_too() {
    let frame = frame_with_rules(&[(300, 400, [140, 140, 140])]);
    let ocr = FakeOcr::new(vec![merchant()]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    assert!(reader.read(&frame, (280, 350), 1.0).unwrap().is_none());
    assert!(reader.read(&frame, (280, 350), 1.0).unwrap().is_none());
    assert_eq!(ocr.reads(), 1);
}

#[test]
fn no_rule_means_no_read() {
    let ocr = FakeOcr::new(vec![robe()]);
    assert!(TooltipReader::new(ocr.clone(), index()).read(&frame_with_rules(&[]), (500, 500), 1.0).unwrap().is_none());
    assert_eq!(ocr.reads(), 0);
}

#[test]
fn reads_the_tooltip_beside_the_cursor_grabbing_only_what_it_needs() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![robe()]), index());
    let found = read_screen(&mut reader, &mut screen, (1000, 900)).unwrap();
    assert_eq!(found.tooltip.item_id, "OccultistRobe_3001");
    let b = &found.tooltip_box;
    assert_eq!((b.left, b.right, b.rule_y), (1030, 1630, 700));
    let grabbed: i64 = screen.grabs.iter().map(|r| i64::from(r.2 - r.0) * i64::from(r.3 - r.1)).sum();
    assert!(grabbed < 3000 * 2000 * 3 / 10);
}

#[test]
fn nothing_beside_the_cursor_means_no_ocr() {
    let mut screen = FakeScreen::new(screen_with_rule(100, 700, GREEN)); // a rule far from the cursor
    let ocr = FakeOcr::new(vec![robe()]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    assert!(read_screen(&mut reader, &mut screen, (2400, 900)).is_none());
    assert_eq!(ocr.reads(), 0);
}

#[test]
fn a_tooltip_read_before_is_not_read_again_from_the_screen() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let ocr = FakeOcr::new(vec![robe()]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    let first = read_screen(&mut reader, &mut screen, (1000, 900));
    let again = read_screen(&mut reader, &mut screen, (1000, 900));
    assert_eq!(first, again);
    assert_eq!(ocr.reads(), 1);
}

#[test]
fn still_showing_while_the_tooltips_rule_is_there() {
    let mut reader = TooltipReader::new(FakeOcr::new(vec![robe()]), index());
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let found = read_screen(&mut reader, &mut screen, (1000, 900)).unwrap();
    assert!(reader.still_showing(&mut screen.grab(), 1.0, SCREEN, &found.tooltip_box));
    let mut gone = FakeScreen::new(Frame::filled(3000, 2000, DARK));
    assert!(!reader.still_showing(&mut gone.grab(), 1.0, SCREEN, &found.tooltip_box));
}

#[test]
fn a_fixed_ui_line_by_the_cursor_is_checked_once_per_rest() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREY));
    let ocr = FakeOcr::new(vec![merchant(); 5]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    for _ in 0..5 {
        assert!(read_screen(&mut reader, &mut screen, (1000, 900)).is_none());
    }
    assert_eq!(ocr.reads(), 1);
    let first_look = screen.grabs.len() - 4;
    assert!(first_look >= 3);
    assert!(screen.grabs[screen.grabs.len() - 4..].iter().all(|r| r.3 - r.1 > 100)); // later looks: one strip grab each
}

#[test]
fn a_tooltip_appearing_where_a_ui_line_was_is_still_read() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREY));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![merchant(), robe()]), index());
    assert!(read_screen(&mut reader, &mut screen, (1000, 900)).is_none());
    screen.frame.fill_rect(1030, 700, 1630, 702, GREEN); // the game draws its tooltip there
    let found = read_screen(&mut reader, &mut screen, (1000, 900)).unwrap();
    assert_eq!(found.tooltip.item_id, "OccultistRobe_3001");
}

#[test]
fn a_new_rest_looks_at_everything_again() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREY));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![merchant(), merchant()]), index());
    read_screen(&mut reader, &mut screen, (1000, 900));
    let before = screen.grabs.len();
    read_screen(&mut reader, &mut screen, (1010, 900));
    assert!(screen.grabs[before..].iter().any(|r| r.3 - r.1 <= 21 && r.2 - r.0 > 64)); // the line measured again
}

#[test]
fn a_ui_line_found_to_be_no_tooltip_at_two_rests_is_skipped_after() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREY));
    let ocr = FakeOcr::new(vec![merchant(); 3]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    read_screen(&mut reader, &mut screen, (1000, 900));
    read_screen(&mut reader, &mut screen, (1010, 905)); // a second rest: miss 2
    let before = screen.grabs.len();
    read_screen(&mut reader, &mut screen, (1005, 910)); // a third rest
    assert_eq!(ocr.reads(), 1); // the same crop: read once
    let crop = TooltipBox { left: 1030, right: 1630, rule_y: 700, color: GREY }.ocr_region(3000, 2000, 1.0);
    assert!(!screen.grabs[before..].contains(&crop)); // now not even grabbed
}

#[test]
fn a_tooltip_that_merely_failed_to_parse_is_read_again_later() {
    let unknown = vec![OcrLine::new("Mystery Blade", 98, 19, 162, 18), OcrLine::new("Rarity: Epic", 101, 387, 150, 15)];
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let ocr = FakeOcr::new(vec![unknown, robe()]);
    let mut reader = TooltipReader::new(ocr.clone(), index());
    assert!(read_screen(&mut reader, &mut screen, (1000, 900)).is_none());
    screen.frame.fill_rect(1100, 1400, 1200, 1480, [60, 60, 60]); // the next capture differs (no cached crop)
    assert!(read_screen(&mut reader, &mut screen, (1010, 905)).is_some());
    assert_eq!(ocr.reads(), 2);
}

#[test]
fn still_showing_when_another_overlay_covers_part_of_the_rule() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![robe()]), index());
    let found = read_screen(&mut reader, &mut screen, (1000, 900)).unwrap();
    screen.frame.fill_rect(1000, 600, 1330, 900, [30, 20, 20]); // e.g. another tool's panel over the left half
    assert!(reader.still_showing(&mut screen.grab(), 1.0, SCREEN, &found.tooltip_box));
}

#[test]
fn a_different_line_where_the_rule_was_is_not_the_tooltip() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![robe()]), index());
    let found = read_screen(&mut reader, &mut screen, (1000, 900)).unwrap();
    screen.frame.fill_rect(0, 690, 3000, 712, DARK);
    screen.frame.fill_rect(900, 700, 1800, 702, GREY); // tooltip gone; a grey UI line happens to be on that row
    assert!(!reader.still_showing(&mut screen.grab(), 1.0, SCREEN, &found.tooltip_box));
}

#[test]
fn one_bad_read_of_a_real_tooltip_does_not_blacklist_its_spot() {
    let garbled = vec![OcrLine::new("Occultist Rabe", 98, 19, 162, 18)]; // the Rarity line missed this time
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREEN));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![garbled, robe()]), index());
    assert!(read_screen(&mut reader, &mut screen, (1000, 900)).is_none());
    screen.frame.fill_rect(1100, 1400, 1200, 1480, [60, 60, 60]); // next rest: a slightly different capture
    assert!(read_screen(&mut reader, &mut screen, (1010, 905)).is_some());
}

#[test]
fn a_tooltip_opening_left_is_not_hidden_by_a_ui_line_on_the_right() {
    let mut frame = Frame::filled(3000, 2000, DARK);
    frame.fill_rect(2150, 700, 2810, 702, GREY); // a UI line crossing the right strip
    let mut screen = FakeScreen::new(frame);
    let mut reader = TooltipReader::new(FakeOcr::new(vec![merchant(), robe()]), index());
    assert!(read_screen(&mut reader, &mut screen, (2000, 900)).is_none());
    screen.frame.fill_rect(1150, 706, 1910, 708, GREEN); // the tooltip opens to the left, 6 px lower
    let found = read_screen(&mut reader, &mut screen, (2000, 900)).unwrap();
    assert!(found.tooltip_box.right <= 1910);
}

#[test]
fn grey_speckle_where_a_common_rule_was_is_not_the_rule() {
    let mut screen = FakeScreen::new(screen_with_rule(1030, 700, GREY));
    let mut reader = TooltipReader::new(FakeOcr::new(vec![robe()]), index());
    let found = read_screen(&mut reader, &mut screen, (1000, 900)).unwrap();
    screen.frame.fill_rect(900, 600, 1800, 900, DARK); // the tooltip closed ...
    let mut seed: u32 = 3;
    for y in 600..900 {
        for x in 900..1800 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if seed >> 24 < 5 {
                // ... over a lightly (2%) speckled background
                screen.frame.set_pixel(x, y, GREY);
            }
        }
    }
    assert!(!reader.still_showing(&mut screen.grab(), 1.0, SCREEN, &found.tooltip_box));
}
