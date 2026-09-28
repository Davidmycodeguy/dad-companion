//! The hover loop's real senses and panel: the cursor, screen and game focus from Windows, the
//! on/off setting, and the card shown in the overlay window.

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use tauri::{AppHandle, Manager, Runtime};

use market::card::TipFacts;
use tooltip::finder::TITLE_HEIGHT_PX;
use tooltip::parser::{ItemIndex, ParsedTooltip};
use tooltip::reader::{Found, TextReader, TooltipReader};
use tooltip::{Frame, OcrLine};

use super::facts;
use super::looper::{HoverLoop, Panel, Reader, Senses};
use super::overlay::{self, Region};
use crate::product;
use crate::state::AppState;

/// Windows, the settings and a monotonic clock.
pub struct LiveSenses<R: Runtime> {
    app: AppHandle<R>,
    started: Instant,
}

impl<R: Runtime> LiveSenses<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app, started: Instant::now() }
    }
}

impl<R: Runtime> Senses for LiveSenses<R> {
    fn enabled(&self) -> bool {
        self.app.try_state::<AppState>().is_some_and(|state| state.hover_values_on())
    }

    fn game_active(&self) -> bool {
        screen::game_has_focus(product::GAME_PROCESS)
    }

    fn cursor(&self) -> (i32, i32) {
        screen::cursor_position().unwrap_or((0, 0))
    }

    fn screen_size(&self) -> (i32, i32) {
        screen::screen_size()
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}

/// Windows' own OCR behind the reader's text-recognition trait.
pub struct WindowsOcr(pub screen::Ocr);

impl TextReader for WindowsOcr {
    fn read(&mut self, frame: &Frame) -> Result<Vec<OcrLine>, String> {
        self.0.read(frame).map_err(|err| err.to_string())
    }
}

/// Finds and re-checks the game's tooltip on the real screen.
pub struct LiveReader {
    reader: TooltipReader<WindowsOcr>,
    started: Instant,
}

impl LiveReader {
    pub fn new(reader: TooltipReader<WindowsOcr>) -> Self {
        Self { reader, started: Instant::now() }
    }
}

/// A region of the screen; a failed capture reads as nothing on screen (the next poll tries again).
fn grab(region: Region) -> Frame {
    screen::grab(region).unwrap_or_else(|err| {
        log::debug!("screen capture failed: {err}");
        Frame::new(0, 0, Vec::new())
    })
}

impl Reader for LiveReader {
    type Found = Found;

    fn read_screen(&mut self, cursor: (i32, i32), scale: f64, screen: (i32, i32)) -> Result<Option<Found>, String> {
        let now = self.started.elapsed().as_secs_f64();
        self.reader.read_screen(&mut grab, cursor, scale, screen, now)
    }

    fn still_showing(&mut self, _cursor: (i32, i32), scale: f64, screen: (i32, i32), found: &Found) -> Result<bool, String> {
        Ok(self.reader.still_showing(&mut grab, scale, screen, &found.tooltip_box))
    }

    fn tooltip_region(&self, found: &Found, scale: f64) -> Region {
        let tooltip = found.tooltip_box;
        let title = (f64::from(TITLE_HEIGHT_PX) * scale).round() as i32;
        (tooltip.left, tooltip.rule_y - title, tooltip.right, tooltip.rule_y)
    }
}

impl ReadTooltip for Found {
    fn parsed(&self) -> &ParsedTooltip {
        &self.tooltip
    }
}

/// Starts hover values on their own thread for the app's lifetime. Without Windows OCR (no OCR
/// language installed) they stay off and say why in the log.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("hover-values".into()).spawn(move || {
        let ocr = match screen::Ocr::new() {
            Ok(ocr) => ocr,
            Err(err) => {
                log::warn!("hover values are off: {err}");
                return;
            }
        };
        let Some(state) = app.try_state::<AppState>() else { return };
        let index = ItemIndex::from_catalog(&state.catalog);
        log::info!("hover values started (Windows OCR in {})", ocr.language());
        let reader = LiveReader::new(TooltipReader::new(WindowsOcr(ocr), index));
        let mut hover = HoverLoop::new(LiveSenses::new(app.clone()), reader, LivePanel::new(app.clone()));
        hover.run(&AtomicBool::new(false));
    });
    if let Err(err) = spawned {
        log::error!("hover values could not start: {err}");
    }
}

/// Builds the card for a tooltip that was read and shows it in the overlay.
pub struct LivePanel<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> LivePanel<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

/// What the loop keeps about a tooltip it read: the parsed text and where the tooltip is.
pub trait ReadTooltip {
    fn parsed(&self) -> &ParsedTooltip;
}

impl<R: Runtime, F: ReadTooltip> Panel<F> for LivePanel<R> {
    fn show(&mut self, found: &F, tooltip: Region, screen: (i32, i32)) -> bool {
        let Some(state) = self.app.try_state::<AppState>() else { return false };
        let parsed = found.parsed();
        let tip = TipFacts {
            title: parsed.title.clone(),
            rarity: parsed.rarity.clone(),
            rolls: parsed.rolls.clone(),
            unread: parsed.unread.len(),
        };
        let card = facts::card_for(&state, &parsed.item_id, &tip, facts::now_s());
        log::debug!("hover card for {} beside {tooltip:?}", parsed.item_id);
        overlay::show(&self.app, &card, tooltip, screen);
        true
    }

    fn hide(&mut self) {
        overlay::hide(&self.app);
    }
}
