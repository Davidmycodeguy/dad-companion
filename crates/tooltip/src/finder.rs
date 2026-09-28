//! Finding the game's item tooltip in screen pixels. Port of DnDTools' `tooltip_finder.py`.
//!
//! Every item tooltip underlines its title with thin rules in the item's rarity colour, running
//! most of the tooltip's width. Nothing else near an item is that long, thin and saturated, so the
//! longest such run nearest the cursor marks the tooltip's width and where its title ends. Pixel
//! sizes are measured at 3840x2160 and scaled by `scale` (screen height / 2160).

use crate::{Frame, Region};

/// An (x, y) position in screen or frame pixels.
pub type Point = (i32, i32);
/// A (width, height) screen or frame size in pixels.
pub type Size = (i32, i32);

/// Shortest rule counted as a tooltip (Gem Ring rule: ~620 px at 4K).
pub const MIN_RULE_PX: i32 = 250;
/// Wider runs are screen borders or UI frames, not tooltips.
pub const MAX_RULE_PX: i32 = 1300;
/// Max - min channel: rarity colors are vivid, text and frames are not.
pub const RULE_MIN_SATURATION: i32 = 50;
pub const RULE_MIN_BRIGHTNESS: i32 = 100;
/// Common tooltips underline the title in grey-white instead: accept bright, colourless runs too
/// (the fill check keeps white text out).
pub const NEUTRAL_MIN_BRIGHTNESS: i32 = 120;
pub const NEUTRAL_MAX_SATURATION: i32 = 40;
/// Poor tooltips underline it in dim grey (~80), as dim as plenty of UI lines, so a dim rule must
/// also have the title rule's shape: two thin lines, the second starting ~5 px below the first (at
/// 4K).
pub const DIM_MIN_BRIGHTNESS: i32 = 60;
pub const DIM_MAX_SATURATION: i32 = 20;
pub const DOUBLE_GAP_PX: (i32, i32) = (4, 6);
/// The second line lit along this much of the first.
pub const DOUBLE_MIN_FILL: f64 = 0.6;
/// The two rules under a title are ~6 px apart.
pub const RULE_GROUP_PX: i32 = 14;
/// A rule fades toward its ends and breaks into pieces up to ~10 px apart.
pub const RULE_MAX_GAP_PX: i32 = 12;
/// A rule row is ~96% lit; a row through bridged text is under 50%.
pub const RULE_MIN_FILL: f64 = 0.8;
/// Title bar above the first rule.
pub const TITLE_HEIGHT_PX: i32 = 110;
/// Text starts ~30 px inside the rule; more would catch neighbouring UI text.
pub const SIDE_PADDING_PX: i32 = 45;
/// Tooltips open beside the hovered item: search this far around the cursor.
pub const SEARCH_X_PX: i32 = 1500;
pub const SEARCH_Y_PX: i32 = 1300;
/// Rules are long and horizontal: every other column is plenty.
pub const COLUMN_STEP: i32 = 2;
/// Tallest tooltip body below the rules.
pub const MAX_BODY_PX: i32 = 1500;
/// For ranking: the hovered item sits beside the tooltip's body, not its rule.
pub const TYPICAL_BODY_PX: i32 = 800;
// Probing: the game opens a tooltip right beside the cursor (~20-30 px away), 600+ px wide, going
// up or down to fit. A narrow strip each side of the cursor therefore crosses the rule of any
// tooltip there, and grabbing two strips is a tiny fraction of grabbing the whole search area.
/// Past the cursor sprite and the rule's faded end, well inside its run.
pub const PROBE_OFFSET_PX: i32 = 200;
pub const PROBE_WIDTH_PX: i32 = 64;
/// A tooltip opening upward ends by the cursor: its rule can be this far up.
pub const PROBE_UP_PX: i32 = 1800;
/// One opening downward starts by the cursor: its rule is a title bar below.
pub const PROBE_DOWN_PX: i32 = 300;
/// A rule lights (nearly) the whole strip; text doesn't.
pub const PROBE_MIN_LIT: f64 = 0.9;
/// Strip columns sampled.
pub const PROBE_COLUMN_STEP: i32 = 4;
/// A rule's two thin lines span ~8 rows; item art lit this wide is a blob.
pub const RULE_MAX_SPAN_PX: i32 = 16;
/// Candidate rules measured per look, nearest the cursor first.
pub const MAX_PROBE_ROWS: usize = 8;
/// Rows either side of a candidate grabbed to measure its whole rule.
pub const BAND_PX: i32 = 10;
/// A tooltip is still up while this much of its rule shows ...
pub const RULE_MIN_VISIBLE: f64 = 0.25;
/// ... in its own colour (its ends fade).
pub const RULE_COLOR_TOLERANCE: i32 = 48;

/// `round()`-like rounding matching Python's `round()` (nearest integer, ties to even).
fn py_round(x: f64) -> i32 {
    x.round_ties_even() as i32
}

/// The common `round(X_PX * scale)` pattern used throughout the Python module.
fn scale_px(px: i32, scale: f64) -> i32 {
    py_round(f64::from(px) * scale)
}

/// A tooltip in frame pixels: the extent of its title rule and the rule's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooltipBox {
    pub left: i32,
    pub right: i32,
    pub rule_y: i32,
    pub color: [u8; 3],
}

impl TooltipBox {
    /// (left, top, right, bottom) to read: the title and everything below, within the frame.
    pub fn ocr_region(&self, frame_width: i32, frame_height: i32, scale: f64) -> Region {
        let pad = scale_px(SIDE_PADDING_PX, scale);
        (
            0.max(self.left - pad),
            0.max(self.rule_y - scale_px(TITLE_HEIGHT_PX, scale)),
            frame_width.min(self.right + pad),
            frame_height.min(self.rule_y + scale_px(MAX_BODY_PX, scale)),
        )
    }
}

/// Number of samples `0, step, 2*step, ...` within `[0, total)` (matches numpy's `::step` slicing).
fn strided_len(total: usize, step: usize) -> usize {
    total.div_ceil(step)
}

/// Fraction of `bits` that is `true` (0.0 for an empty slice, which none of our callers pass).
fn mean_true(bits: &[bool]) -> f64 {
    if bits.is_empty() {
        return 0.0;
    }
    bits.iter().filter(|&&b| b).count() as f64 / bits.len() as f64
}

/// A row-major boolean grid over a probed window or strip: this crate's stand-in for a numpy
/// boolean mask, needed because a window can be column-subsampled and so isn't a plain `Frame`.
struct Mask {
    width: usize,
    height: usize,
    bits: Vec<bool>,
}

impl Mask {
    fn new(width: usize, height: usize) -> Self {
        Self { width, height, bits: vec![false; width * height] }
    }

    fn get(&self, x: usize, y: usize) -> bool {
        self.bits[y * self.width + x]
    }

    fn set(&mut self, x: usize, y: usize, value: bool) {
        self.bits[y * self.width + x] = value;
    }

    fn row(&self, y: usize) -> &[bool] {
        &self.bits[y * self.width..(y + 1) * self.width]
    }
}

/// (vivid, neutral) masks over `width` x `height` pixels of `frame` starting at (`x0`, `y0`),
/// sampling every `step` columns: rarity-coloured pixels, and bright colourless ones.
fn rule_masks(frame: &Frame, x0: usize, y0: usize, width: usize, height: usize, step: usize) -> (Mask, Mask) {
    let mut vivid = Mask::new(width, height);
    let mut neutral = Mask::new(width, height);
    for row in 0..height {
        for col in 0..width {
            let [r, g, b] = frame.pixel(x0 + col * step, y0 + row);
            let hi = i32::from(r.max(g).max(b));
            let lo = i32::from(r.min(g).min(b));
            let saturation = hi - lo;
            vivid.set(col, row, saturation >= RULE_MIN_SATURATION && hi >= RULE_MIN_BRIGHTNESS);
            neutral.set(col, row, saturation < NEUTRAL_MAX_SATURATION && hi >= NEUTRAL_MIN_BRIGHTNESS);
        }
    }
    (vivid, neutral)
}

/// Dim, colourless pixels: a Poor rule's grey, below the Common rule's brightness. Same window
/// convention as `rule_masks`.
fn dim_mask(frame: &Frame, x0: usize, y0: usize, width: usize, height: usize, step: usize) -> Mask {
    let mut mask = Mask::new(width, height);
    for row in 0..height {
        for col in 0..width {
            let [r, g, b] = frame.pixel(x0 + col * step, y0 + row);
            let hi = i32::from(r.max(g).max(b));
            let lo = i32::from(r.min(g).min(b));
            mask.set(col, row, hi - lo < DIM_MAX_SATURATION && (DIM_MIN_BRIGHTNESS..NEUTRAL_MIN_BRIGHTNESS).contains(&hi));
        }
    }
    mask
}

/// [(start, end)] of runs of `true` at least `min_length` long, bridging gaps up to `max_gap`, that
/// stay at least `min_fill` lit (text bridged across letter gaps is mostly gaps).
fn runs(row: &[bool], min_length: usize, max_gap: usize, min_fill: f64) -> Vec<(usize, usize)> {
    let mut raw = Vec::new();
    let mut start = None;
    for (i, &lit) in row.iter().enumerate() {
        match (lit, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                raw.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        raw.push((s, row.len()));
    }
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in raw {
        match merged.last_mut() {
            Some(last) if start - last.1 <= max_gap => last.1 = end,
            _ => merged.push((start, end)),
        }
    }
    merged.into_iter().filter(|&(a, b)| b - a >= min_length && mean_true(&row[a..b]) >= min_fill).collect()
}

/// One rule accumulated across rows by `rules_from_mask`: its first row, current horizontal extent,
/// and the last row it was seen on (rows a rule's two faded lines break into are merged together).
struct RuleAcc {
    top: usize,
    left: usize,
    right: usize,
    last_row: usize,
}

/// [(top, left, right)] of every title rule: long runs of `mask`, merged across nearby rows.
///
/// A rule is drawn as two thin lines whose faded ends break into pieces on neighbouring rows, so
/// runs a few rows apart that overlap sideways are one rule. Tooltips can sit side by side
/// (comparisons), so one row may hold several rules. The row-proximity threshold here is the raw
/// `RULE_GROUP_PX`, unscaled, as in Python.
fn rules_from_mask(mask: &Mask, min_length: usize, max_gap: usize, max_length: usize) -> Vec<(usize, usize, usize)> {
    let group_rows = RULE_GROUP_PX as usize;
    let mut rules: Vec<RuleAcc> = Vec::new();
    for y in 0..mask.height {
        let row = mask.row(y);
        if row.iter().filter(|&&b| b).count() < min_length {
            continue;
        }
        for (left, right) in runs(row, min_length, max_gap, RULE_MIN_FILL) {
            if right - left > max_length {
                continue;
            }
            let existing = rules.iter_mut().find(|r| y - r.last_row <= group_rows && left < r.right && right > r.left);
            match existing {
                Some(r) => {
                    r.left = r.left.min(left);
                    r.right = r.right.max(right);
                    r.last_row = y;
                }
                None => rules.push(RuleAcc { top: y, left, right, last_row: y }),
            }
        }
    }
    rules.into_iter().map(|r| (r.top, r.left, r.right)).collect()
}

/// Rows below a rule's first line where its second line starts, for `scale` (matches Python's
/// `_gaps`).
fn double_gaps(scale: f64) -> std::ops::RangeInclusive<usize> {
    let low = 2i32.max(scale_px(DOUBLE_GAP_PX.0, scale)) as usize;
    let high = 2i32.max(scale_px(DOUBLE_GAP_PX.1, scale)) as usize;
    low..=low.max(high)
}

/// Whether a dim rule (top, left, right) has a second line just below it, as title rules do.
fn is_double(dim: &Mask, rule: (usize, usize, usize), scale: f64) -> bool {
    let (top, left, right) = rule;
    double_gaps(scale).any(|gap| {
        let row = top + gap;
        row < dim.height && mean_true(&dim.row(row)[left..right]) >= DOUBLE_MIN_FILL
    })
}

/// Squared distance from `point` to the nearest point of the rectangle [left,right] x [top,bottom]
/// (0 when the point is inside). `point`'s x can be fractional (a cursor position scaled down by
/// the column sampling step).
fn distance_sq(point: (f64, f64), left: i32, top: i32, right: i32, bottom: i32) -> f64 {
    let (x, y) = point;
    let dx = (f64::from(left) - x).max(0.0).max(x - f64::from(right));
    let dy = (f64::from(top) - y).max(0.0).max(y - f64::from(bottom));
    dx * dx + dy * dy
}

/// Screen regions (left, top, right, bottom) to watch for a title rule: a narrow strip each side of
/// the cursor, clipped to the screen (a strip mostly off screen is dropped).
pub fn probe_strips(cursor: Point, scale: f64, screen: Size) -> Vec<Region> {
    let (cx, cy) = cursor;
    let (width, height) = screen;
    let offset = scale_px(PROBE_OFFSET_PX, scale);
    let strip = 8.max(scale_px(PROBE_WIDTH_PX, scale));
    let top = 0.max(cy - scale_px(PROBE_UP_PX, scale));
    let bottom = height.min(cy + scale_px(PROBE_DOWN_PX, scale));
    let mut regions = Vec::new();
    for start in [cx + offset, cx - offset - strip] {
        let left = 0.max(start);
        let right = width.min(start + strip);
        if right - left >= strip / 2 && bottom > top {
            regions.push((left, top, right, bottom));
        }
    }
    regions
}

/// Rows below a rule's first line where its second (dim) line starts across the whole strip: a dim
/// line, dark rows, a second dim line. `strip` is already column-sampled, as `rule_rows` leaves it.
fn double_dim_rows(strip: &Frame, sampled_width: usize, scale: f64, step: usize) -> Vec<usize> {
    let dim = dim_mask(strip, 0, 0, sampled_width, strip.height(), step);
    let lit: Vec<bool> = (0..strip.height()).map(|y| mean_true(dim.row(y)) >= PROBE_MIN_LIT).collect();
    let gaps = double_gaps(scale);
    let last_gap = *gaps.end();
    /// Rows past the second dim line checked to still be dark, confirming it's a thin line, not a
    /// filled area (Python's inline `3`, scaled the same way as the gap constants).
    const AFTER_PX: i32 = 3;
    let after = 2usize.max(scale_px(AFTER_PX, scale) as usize);
    let mut rows = Vec::new();
    for y in 0..lit.len() {
        if !lit[y] || y == 0 || lit[y - 1] || y + last_gap + after >= lit.len() {
            continue;
        }
        for gap in gaps.clone() {
            let bridged_solid = lit[y + 1..y + gap].iter().all(|&b| b);
            if lit[y + gap] && !bridged_solid && !lit[y + gap + after] {
                rows.push(y);
                break;
            }
        }
    }
    rows
}

/// Rows of `strip` crossed end to end by a title rule: the top row of each thin lit cluster.
pub fn rule_rows(strip: &Frame, scale: f64) -> Vec<i32> {
    if strip.width() == 0 || strip.height() == 0 {
        return Vec::new();
    }
    let step = PROBE_COLUMN_STEP as usize;
    let sampled_width = strided_len(strip.width(), step);
    let (vivid, neutral) = rule_masks(strip, 0, 0, sampled_width, strip.height(), step);
    let lit_rows: Vec<usize> =
        (0..strip.height()).filter(|&y| mean_true_or(vivid.row(y), neutral.row(y)) >= PROBE_MIN_LIT).collect();
    let group = 2usize.max(scale_px(RULE_GROUP_PX, scale) as usize);
    let span = 4usize.max(scale_px(RULE_MAX_SPAN_PX, scale) as usize);
    let mut clusters: Vec<(usize, usize)> = Vec::new();
    for y in lit_rows {
        match clusters.last_mut() {
            Some(last) if y - last.1 <= group => last.1 = y,
            _ => clusters.push((y, y)),
        }
    }
    let rows: Vec<usize> = clusters.into_iter().filter(|&(top, bottom)| bottom - top < span).map(|(top, _)| top).collect();
    let mut all_rows = rows.clone();
    for y in double_dim_rows(strip, sampled_width, scale, step) {
        if rows.iter().all(|&r| y.abs_diff(r) > group) {
            all_rows.push(y);
        }
    }
    all_rows.sort_unstable();
    all_rows.into_iter().map(|y| y as i32).collect()
}

/// Fraction of rows where `a` or `b` is lit (elementwise OR, then mean).
fn mean_true_or(a: &[bool], b: &[bool]) -> f64 {
    if a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).filter(|&(&x, &y)| x || y).count() as f64 / a.len() as f64
}

/// A simple deterministic 64-bit hash (FNV-1a) of a pixel patch. Stands in for Python's sha1
/// digest: only used so a caller can recognise the same patch of pixels again, never for security,
/// and equal bytes always hash equal.
fn hash_bytes(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(PRIME))
}

/// A rule row candidate from `probe`, nearest the cursor first: the screen row it crosses, and a
/// signature of the strip patch around it. Equal pixels give an equal signature, so a caller can
/// skip a row it already found to be no tooltip until those pixels change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowSignature {
    strip: usize,
    row: i32,
    hash: u64,
}

/// Sort key for a rule row: how far the cursor is from where that tooltip's body would be (a
/// tooltip opening upward has its rule far above the item it describes).
fn row_nearness(cursor: Point, scale: f64) -> impl Fn(i32) -> i32 {
    let cy = cursor.1;
    let title = scale_px(TITLE_HEIGHT_PX, scale);
    let body = scale_px(TYPICAL_BODY_PX, scale);
    move |row: i32| (row - title - cy).max(0).max(cy - (row + body))
}

/// [(screen row, signature)] of rule candidates crossing the probe strips, likeliest first. The
/// signature fingerprints what that strip shows around the row, so a caller can skip a row it
/// already found to be no tooltip (a fixed UI line) until those pixels change. Each strip keeps its
/// own rows: a line on one side must not hide a tooltip opening on the other.
pub fn probe(grab: &mut dyn FnMut(Region) -> Frame, cursor: Point, scale: f64, screen: Size) -> Vec<(i32, RowSignature)> {
    let below = 8usize.max(scale_px(2 * RULE_MAX_SPAN_PX, scale) as usize);
    let strips = probe_strips(cursor, scale, screen);
    let Some(&first) = strips.first() else {
        return Vec::new();
    };
    // One grab for both strips: a screen copy waits for the next composited frame (~15 ms at 60 Hz)
    // whatever its size, so the number of grabs is what costs, not their width.
    let (mut box_left, box_top, mut box_right, box_bottom) = first;
    for &(left, _, right, _) in &strips[1..] {
        box_left = box_left.min(left);
        box_right = box_right.max(right);
    }
    let both = grab((box_left, box_top, box_right, box_bottom));
    let mut rows = Vec::new();
    for (index, &(left, top, right, _)) in strips.iter().enumerate() {
        let pixels = both.crop((left - box_left) as usize, 0, (right - box_left) as usize, both.height());
        for row in rule_rows(&pixels, scale) {
            let row_usize = row as usize;
            let patch = pixels.crop(0, row_usize.saturating_sub(2), pixels.width(), row_usize + below);
            let absolute_row = top + row;
            let signature = RowSignature { strip: index, row: absolute_row, hash: hash_bytes(patch.data()) };
            rows.push((absolute_row, signature));
        }
    }
    let near = row_nearness(cursor, scale);
    rows.sort_by_key(|&(row, _)| near(row));
    rows
}

/// TooltipBoxes (screen pixels) of the rules on `row`, from a thin band grabbed across the search
/// width: their full extent and colour.
pub fn measure(
    grab: &mut dyn FnMut(Region) -> Frame,
    row: i32,
    cursor: Point,
    scale: f64,
    screen: Size,
    limit: usize,
) -> Vec<TooltipBox> {
    let (cx, cy) = cursor;
    let (width, height) = screen;
    let reach = scale_px(SEARCH_X_PX, scale);
    let band = 4.max(scale_px(BAND_PX, scale));
    let region = (0.max(cx - reach), 0.max(row - band), width.min(cx + reach), height.min(row + band + 1));
    let pixels = grab(region);
    if pixels.width() == 0 || pixels.height() == 0 {
        return Vec::new();
    }
    // Rules far above the cursor still count: clamp, don't drop.
    let local = (cx - region.0, (cy - region.1).clamp(0, pixels.height() as i32 - 1));
    find_tooltips(&pixels, local, scale, limit)
        .into_iter()
        .map(|b| TooltipBox { left: b.left + region.0, right: b.right + region.0, rule_y: b.rule_y + region.1, color: b.color })
        .collect()
}

/// Sort key: how far a box's tooltip is from the cursor (its body beside the item, not just its
/// rule).
pub fn nearness(cursor: Point, scale: f64) -> impl Fn(&TooltipBox) -> f64 {
    let title = scale_px(TITLE_HEIGHT_PX, scale);
    let body = scale_px(TYPICAL_BODY_PX, scale);
    let point = (f64::from(cursor.0), f64::from(cursor.1));
    move |b: &TooltipBox| distance_sq(point, b.left, b.rule_y - title, b.right, b.rule_y + body)
}

/// Boxes nearest the cursor first.
pub fn rank(mut boxes: Vec<TooltipBox>, cursor: Point, scale: f64) -> Vec<TooltipBox> {
    let near = nearness(cursor, scale);
    boxes.sort_by(|a, b| near(a).total_cmp(&near(b)));
    boxes
}

/// Up to `limit` TooltipBoxes (screen pixels) by the cursor, nearest first. Grabs only the probe
/// strips and, for each rule they cross, a thin band to measure it: a few % of the screen.
/// `grab(region)` returns that region's pixels as an RGB frame.
pub fn find_near_cursor(
    grab: &mut dyn FnMut(Region) -> Frame,
    cursor: Point,
    scale: f64,
    screen: Size,
    limit: usize,
) -> Vec<TooltipBox> {
    let group = 2.max(scale_px(RULE_GROUP_PX, scale));
    let mut boxes: Vec<TooltipBox> = Vec::new();
    for (row, _) in probe(grab, cursor, scale, screen).into_iter().take(MAX_PROBE_ROWS) {
        for candidate in measure(grab, row, cursor, scale, screen, limit) {
            let overlaps = boxes.iter().any(|b| {
                (candidate.rule_y - b.rule_y).abs() <= group && candidate.left < b.right && candidate.right > b.left
            });
            if !overlaps {
                boxes.push(candidate);
            }
        }
    }
    let mut ranked = rank(boxes, cursor, scale);
    ranked.truncate(limit);
    ranked
}

/// Whether every channel of `pixel` is within `tolerance` of `color`.
fn channel_diff_within(pixel: [u8; 3], color: [u8; 3], tolerance: i32) -> bool {
    (0..3).all(|i| (i32::from(pixel[i]) - i32::from(color[i])).abs() <= tolerance)
}

/// Whether a band grabbed over a tooltip's rule (exactly its extent) still shows it: enough of its
/// columns lit in the rule's colour. Another overlay may cover part of it, so not all of it.
pub fn rule_still_there(pixels: &Frame, color: [u8; 3]) -> bool {
    let (width, height) = (pixels.width(), pixels.height());
    if width == 0 || height == 0 {
        return false;
    }
    let (vivid, neutral) = rule_masks(pixels, 0, 0, width, height, 1);
    let dim = dim_mask(pixels, 0, 0, width, height, 1);
    let mut best = 0.0f64;
    for y in 0..height {
        let lit = (0..width)
            .filter(|&x| {
                (vivid.get(x, y) || neutral.get(x, y) || dim.get(x, y)) && channel_diff_within(pixels.pixel(x, y), color, RULE_COLOR_TOLERANCE)
            })
            .count();
        // Along one row: another overlay may hide part of the rule, but scattered specks aren't one.
        best = best.max(lit as f64 / width as f64);
    }
    best >= RULE_MIN_VISIBLE
}

/// The pixel at window column `col`, row `row` (window coordinates, before the `step` column
/// sampling and the `(x0, y0)` offset are applied).
fn window_pixel(frame: &Frame, x0: i32, y0: i32, step: usize, col: usize, row: usize) -> [u8; 3] {
    let x = x0 + (col * step) as i32;
    let y = y0 + row as i32;
    frame.pixel(x as usize, y as usize)
}

/// The median of `values`, truncated toward zero like Python's `int(np.median(...))`: for an even
/// count that's the floor of the average of the two middle values, since both are non-negative.
fn channel_median_int(mut values: Vec<u8>) -> u8 {
    values.sort_unstable();
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        ((u32::from(values[n / 2 - 1]) + u32::from(values[n / 2])) / 2) as u8
    }
}

/// The rule's colour at (`row`, `left..right`): the median RGB of its lit pixels (vivid or neutral,
/// falling back to dim), or black when nothing in that span is lit.
#[allow(clippy::too_many_arguments)]
fn rule_color(
    vivid: &Mask,
    neutral: &Mask,
    dim: &Mask,
    frame: &Frame,
    x0: i32,
    y0: i32,
    step: usize,
    row: usize,
    left: usize,
    right: usize,
) -> [u8; 3] {
    let mut cols: Vec<usize> = (left..right).filter(|&c| vivid.get(c, row) || neutral.get(c, row)).collect();
    if cols.is_empty() {
        cols = (left..right).filter(|&c| dim.get(c, row)).collect();
    }
    if cols.is_empty() {
        return [0, 0, 0];
    }
    let (mut r, mut g, mut b) = (Vec::with_capacity(cols.len()), Vec::with_capacity(cols.len()), Vec::with_capacity(cols.len()));
    for c in cols {
        let [pr, pg, pb] = window_pixel(frame, x0, y0, step, c, row);
        r.push(pr);
        g.push(pg);
        b.push(pb);
    }
    [channel_median_int(r), channel_median_int(g), channel_median_int(b)]
}

/// The TooltipBox nearest `cursor` (frame pixels), or None when no tooltip is showing.
pub fn find_tooltip(frame: &Frame, cursor: Point, scale: f64) -> Option<TooltipBox> {
    find_tooltips(frame, cursor, scale, 1).into_iter().next()
}

/// Up to `limit` TooltipBoxes, nearest `cursor` first. Fixed UI lines can look like title rules, so
/// callers confirm each by reading it.
pub fn find_tooltips(frame: &Frame, cursor: Point, scale: f64, limit: usize) -> Vec<TooltipBox> {
    let (cx, cy) = cursor;
    let (fw, fh) = (frame.width() as i32, frame.height() as i32);
    let x0 = 0.max(cx - scale_px(SEARCH_X_PX, scale));
    let y0 = 0.max(cy - scale_px(SEARCH_Y_PX, scale));
    // Python's numpy slicing clips a too-large stop index to the frame's edge; a negative one (an
    // off-screen-to-the-left/above cursor) would instead wrap from the end, which no test relies on
    // and which we treat as simply empty here.
    let x1 = fw.min(cx + scale_px(SEARCH_X_PX, scale)).max(x0);
    let y1 = fh.min(cy + scale_px(SEARCH_Y_PX, scale)).max(y0);
    let step = COLUMN_STEP as usize;
    let win_w = strided_len((x1 - x0) as usize, step);
    let win_h = (y1 - y0) as usize;

    let (vivid, neutral) = rule_masks(frame, x0 as usize, y0 as usize, win_w, win_h, step);
    let dim = dim_mask(frame, x0 as usize, y0 as usize, win_w, win_h, step);

    let step_f = f64::from(COLUMN_STEP);
    let widest = py_round(f64::from(MAX_RULE_PX) * scale / step_f) as usize;
    let shortest = 10usize.max(py_round(f64::from(MIN_RULE_PX) * scale / step_f) as usize);
    let gap = 1usize.max(py_round(f64::from(RULE_MAX_GAP_PX) * scale / step_f) as usize);

    let mut candidates = rules_from_mask(&vivid, shortest, gap, widest);
    candidates.extend(rules_from_mask(&neutral, shortest, gap, widest));
    candidates.extend(rules_from_mask(&dim, shortest, gap, widest).into_iter().filter(|&r| is_double(&dim, r, scale)));

    let title = scale_px(TITLE_HEIGHT_PX, scale);
    let body = scale_px(TYPICAL_BODY_PX, scale);
    let local = (f64::from(cx - x0) / step_f, f64::from(cy - y0));

    let mut keyed: Vec<((usize, usize, usize), f64)> = candidates
        .into_iter()
        .map(|r @ (top, left, right)| (r, distance_sq(local, left as i32, top as i32 - title, right as i32, top as i32 + body)))
        .collect();
    keyed.sort_by(|a, b| a.1.total_cmp(&b.1));
    keyed.truncate(limit);

    keyed
        .into_iter()
        .map(|((y, left, right), _)| TooltipBox {
            left: (left * step) as i32 + x0,
            right: (right * step) as i32 + x0,
            rule_y: y as i32 + y0,
            color: rule_color(&vivid, &neutral, &dim, frame, x0, y0, step, y, left, right),
        })
        .collect()
}
