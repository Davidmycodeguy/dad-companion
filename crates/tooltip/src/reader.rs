//! Reads the item tooltip under the cursor: find it, OCR it, parse it, remembering screens read.
//! Port of DnDTools' `hover_reader.py`.
//!
//! Called over and over while the cursor rests. Rows that turned out to be fixed UI lines are
//! skipped until their pixels change, and a fixed UI line found to hold no tooltip at separate
//! rests is not even grabbed again for a while, so a look usually costs two strip grabs.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use crate::finder::{self, RowSignature, TooltipBox, MAX_PROBE_ROWS, RULE_GROUP_PX, TITLE_HEIGHT_PX};
use crate::frame::{Frame, OcrLine, Region};
use crate::parser::{looks_like_tooltip, parse_tooltip, ItemIndex, ParsedTooltip};

/// Fixed UI lines can look like title rules; each costs one ~25 ms read.
pub const MAX_CANDIDATES: usize = 3;
/// Screens already read (a tooltip stays up while the cursor rests).
const CACHE_SIZE: usize = 64;
/// Fixed UI lines found to be no tooltip, remembered by where they are ...
const NOT_TOOLTIPS_SIZE: usize = 256;
/// ... once this many rests found no tooltip text there (one bad read proves nothing) ...
const UI_LINE_MISSES: u32 = 2;
/// ... and forgotten after this long (screens change).
const UI_LINE_MEMORY_S: f64 = 600.0;
/// 4K text is read best at half size; smaller screens at full size.
const OCR_BASE_SCALE: f64 = 0.5;
/// Side of the grey thumbnail a crop is recognised by.
const SIGNATURE_SIZE: usize = 48;

/// Turns pixels into lines of text (Windows OCR in the app, a fake in tests).
pub trait TextReader {
    fn read(&mut self, frame: &Frame) -> Result<Vec<OcrLine>, String>;
}

/// A tooltip read from the screen: its text and where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub tooltip: ParsedTooltip,
    pub tooltip_box: TooltipBox,
}

/// What reading one crop gave: the tooltip, and whether the text could be a tooltip at all.
type CropResult = (Option<ParsedTooltip>, bool);

/// A rule's place, loosely (measurements wobble by a pixel or two): row, ends and colour.
type Place = (i32, i32, i32, [u8; 3]);

/// A rest of the cursor: where it is, the scale (as bits, to compare exactly) and the screen size.
type Rest = ((i32, i32), u64, (i32, i32));

pub struct TooltipReader<T> {
    ocr: T,
    index: ItemIndex,
    candidates: usize,
    /// Crop signature -> what it read, oldest first.
    cache: HashMap<u64, CropResult>,
    cache_order: VecDeque<u64>,
    /// (cursor, scale, screen) of the rest being looked at.
    rest: Option<Rest>,
    /// Rests seen, so misses at a spot count once per rest.
    rests: u64,
    /// Probe rows found to be no tooltip during this rest.
    rejected: HashSet<RowSignature>,
    /// Rule place -> (misses, last rest, when) with no "Rarity:" line under it, oldest first.
    not_tooltips: HashMap<Place, (u32, u64, f64)>,
    not_tooltips_order: VecDeque<Place>,
}

impl<T: TextReader> TooltipReader<T> {
    pub fn new(ocr: T, index: ItemIndex) -> Self {
        Self {
            ocr,
            index,
            candidates: MAX_CANDIDATES,
            cache: HashMap::new(),
            cache_order: VecDeque::new(),
            rest: None,
            rests: 0,
            rejected: HashSet::new(),
            not_tooltips: HashMap::new(),
            not_tooltips_order: VecDeque::new(),
        }
    }

    /// The tooltip nearest `cursor` in a whole frame, or None.
    pub fn read(&mut self, frame: &Frame, cursor: (i32, i32), scale: f64) -> Result<Option<Found>, String> {
        for tooltip_box in finder::find_tooltips(frame, cursor, scale, self.candidates) {
            let region = tooltip_box.ocr_region(frame.width() as i32, frame.height() as i32, scale);
            let crop = crop_region(frame, region);
            if let (Some(tooltip), _) = self.read_crop(&crop, scale)? {
                return Ok(Some(Found { tooltip, tooltip_box }));
            }
        }
        Ok(None)
    }

    /// The tooltip beside `cursor`, grabbing only the screen regions needed, or None. `grab`
    /// returns a region's pixels; `now` is a monotonic clock in seconds; the box is in screen
    /// pixels.
    pub fn read_screen(
        &mut self,
        grab: &mut dyn FnMut(Region) -> Frame,
        cursor: (i32, i32),
        scale: f64,
        screen: (i32, i32),
        now: f64,
    ) -> Result<Option<Found>, String> {
        let rest = (cursor, scale.to_bits(), screen);
        if self.rest != Some(rest) {
            self.rest = Some(rest);
            self.rejected.clear();
            self.rests += 1;
        }
        let rows: Vec<(i32, RowSignature)> = finder::probe(grab, cursor, scale, screen)
            .into_iter()
            .filter(|(_, signature)| !self.rejected.contains(signature))
            .take(MAX_PROBE_ROWS)
            .collect();
        let mut candidates: Vec<(TooltipBox, RowSignature)> = Vec::new();
        for (row, signature) in rows {
            let boxes = finder::measure(grab, row, cursor, scale, screen, self.candidates);
            if boxes.is_empty() {
                self.rejected.insert(signature);
            }
            candidates.extend(boxes.into_iter().map(|b| (b, signature)));
        }
        let near = finder::nearness(cursor, scale);
        candidates.sort_by(|a, b| near(&a.0).total_cmp(&near(&b.0)));
        let mut failed = HashSet::new();
        for (tooltip_box, signature) in candidates.iter().take(self.candidates) {
            let place = place(tooltip_box);
            if self.known_ui_line(&place, now) {
                // A fixed UI line seen before: no grab, no OCR.
                failed.insert(*signature);
                continue;
            }
            let crop = grab(tooltip_box.ocr_region(screen.0, screen.1, scale));
            let (parsed, tooltip_like) = self.read_crop(&crop, scale)?;
            if let Some(tooltip) = parsed {
                return Ok(Some(Found { tooltip, tooltip_box: *tooltip_box }));
            }
            if !tooltip_like {
                self.note_miss(place, now);
            }
            failed.insert(*signature);
        }
        let later: HashSet<RowSignature> = candidates.iter().skip(self.candidates).map(|(_, s)| *s).collect();
        self.rejected.extend(failed.difference(&later));
        Ok(None)
    }

    fn known_ui_line(&self, place: &Place, now: f64) -> bool {
        self.not_tooltips
            .get(place)
            .is_some_and(|(misses, _, when)| *misses >= UI_LINE_MISSES && now - when < UI_LINE_MEMORY_S)
    }

    /// No tooltip text under this rule; after misses at separate rests it counts as a fixed UI line.
    fn note_miss(&mut self, place: Place, now: f64) {
        let (mut misses, last_rest, when) = self.not_tooltips.get(&place).copied().unwrap_or((0, 0, f64::NEG_INFINITY));
        if now - when >= UI_LINE_MEMORY_S {
            misses = 0;
        }
        if last_rest != self.rests {
            misses += 1;
        }
        self.not_tooltips.insert(place, (misses, self.rests, now));
        self.not_tooltips_order.retain(|p| *p != place);
        self.not_tooltips_order.push_back(place);
        if self.not_tooltips_order.len() > NOT_TOOLTIPS_SIZE {
            if let Some(oldest) = self.not_tooltips_order.pop_front() {
                self.not_tooltips.remove(&oldest);
            }
        }
    }

    /// Whether the tooltip `tooltip_box` (screen pixels) is still up: one thin grab over its rule.
    pub fn still_showing(
        &mut self,
        grab: &mut dyn FnMut(Region) -> Frame,
        scale: f64,
        screen: (i32, i32),
        tooltip_box: &TooltipBox,
    ) -> bool {
        let band = 2.max((RULE_GROUP_PX as f64 * scale).round() as i32);
        let pixels = grab((
            tooltip_box.left,
            (tooltip_box.rule_y - band).max(0),
            tooltip_box.right,
            screen.1.min(tooltip_box.rule_y + band + 1),
        ));
        finder::rule_still_there(&pixels, tooltip_box.color)
    }

    /// What a tooltip-sized crop reads as, from the cache when this exact crop was read before.
    fn read_crop(&mut self, crop: &Frame, scale: f64) -> Result<CropResult, String> {
        let key = signature(crop);
        if let Some(result) = self.cache.get(&key) {
            let result = result.clone();
            self.cache_order.retain(|k| *k != key);
            self.cache_order.push_back(key);
            return Ok(result);
        }
        let factor = if scale > 0.0 { (OCR_BASE_SCALE / scale).min(1.0) } else { 1.0 };
        let small = if factor == 0.5 {
            // 4K: a 2x2 box average, fast and as readable as a smooth resize.
            reduce_by_two(crop)
        } else if factor < 1.0 {
            let width = ((crop.width() as f64 * factor).round() as usize).max(1);
            let height = ((crop.height() as f64 * factor).round() as usize).max(1);
            resize_bilinear(crop, width, height)
        } else {
            crop.clone()
        };
        let lines = self.ocr.read(&small)?;
        let title_bottom = TITLE_HEIGHT_PX as f64 * scale * factor;
        let result = (parse_tooltip(&lines, &self.index, title_bottom), looks_like_tooltip(&lines));
        self.cache.insert(key, result.clone());
        self.cache_order.push_back(key);
        if self.cache_order.len() > CACHE_SIZE {
            if let Some(oldest) = self.cache_order.pop_front() {
                self.cache.remove(&oldest);
            }
        }
        Ok(result)
    }
}

fn place(tooltip_box: &TooltipBox) -> Place {
    let [r, g, b] = tooltip_box.color;
    (tooltip_box.rule_y.div_euclid(4), tooltip_box.left.div_euclid(8), tooltip_box.right.div_euclid(8), [r / 16, g / 16, b / 16])
}

/// The part of `frame` inside `region` (frame pixels), clipped to the frame.
fn crop_region(frame: &Frame, (left, top, right, bottom): Region) -> Frame {
    let clamp = |v: i32, max: usize| (v.max(0) as usize).min(max);
    frame.crop(clamp(left, frame.width()), clamp(top, frame.height()), clamp(right, frame.width()), clamp(bottom, frame.height()))
}

/// A crop's identity: a hash of its grey thumbnail, so the same tooltip on screen reads once.
fn signature(frame: &Frame) -> u64 {
    let mut hasher = DefaultHasher::new();
    (frame.width(), frame.height()).hash(&mut hasher);
    if frame.width() > 0 && frame.height() > 0 {
        for ty in 0..SIGNATURE_SIZE {
            let (y0, y1) = span(ty, frame.height());
            for tx in 0..SIGNATURE_SIZE {
                let (x0, x1) = span(tx, frame.width());
                let mut sum = 0u64;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let [r, g, b] = frame.pixel(x, y);
                        sum += (299 * u64::from(r) + 587 * u64::from(g) + 114 * u64::from(b)) / 1000;
                    }
                }
                let count = ((y1 - y0) * (x1 - x0)).max(1) as u64;
                ((sum / count) as u8).hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// The source pixels [start, end) that thumbnail cell `cell` of `SIGNATURE_SIZE` covers.
fn span(cell: usize, length: usize) -> (usize, usize) {
    let start = cell * length / SIGNATURE_SIZE;
    let end = ((cell + 1) * length / SIGNATURE_SIZE).max(start + 1).min(length);
    (start.min(length.saturating_sub(1)), end)
}

/// Half size: each output pixel is the average of a 2x2 block (odd edges dropped, like PIL's reduce).
fn reduce_by_two(frame: &Frame) -> Frame {
    let (width, height) = ((frame.width() / 2).max(1), (frame.height() / 2).max(1));
    if frame.width() < 2 || frame.height() < 2 {
        return frame.clone();
    }
    let mut data = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let px = [frame.pixel(2 * x, 2 * y), frame.pixel(2 * x + 1, 2 * y), frame.pixel(2 * x, 2 * y + 1), frame.pixel(2 * x + 1, 2 * y + 1)];
            for c in 0..3 {
                let sum: u32 = px.iter().map(|p| u32::from(p[c])).sum();
                data.push(((sum + 2) / 4) as u8);
            }
        }
    }
    Frame::new(width, height, data)
}

/// A smooth resize to `width` x `height`.
fn resize_bilinear(frame: &Frame, width: usize, height: usize) -> Frame {
    let (sw, sh) = (frame.width(), frame.height());
    if sw == 0 || sh == 0 {
        return Frame::new(0, 0, Vec::new());
    }
    let mut data = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        let fy = ((y as f64 + 0.5) * sh as f64 / height as f64 - 0.5).clamp(0.0, (sh - 1) as f64);
        let (y0, wy) = (fy.floor() as usize, fy - fy.floor());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..width {
            let fx = ((x as f64 + 0.5) * sw as f64 / width as f64 - 0.5).clamp(0.0, (sw - 1) as f64);
            let (x0, wx) = (fx.floor() as usize, fx - fx.floor());
            let x1 = (x0 + 1).min(sw - 1);
            let (a, b, c, d) = (frame.pixel(x0, y0), frame.pixel(x1, y0), frame.pixel(x0, y1), frame.pixel(x1, y1));
            for ch in 0..3 {
                let top = f64::from(a[ch]) * (1.0 - wx) + f64::from(b[ch]) * wx;
                let bottom = f64::from(c[ch]) * (1.0 - wx) + f64::from(d[ch]) * wx;
                data.push((top * (1.0 - wy) + bottom * wy).round() as u8);
            }
        }
    }
    Frame::new(width, height, data)
}
