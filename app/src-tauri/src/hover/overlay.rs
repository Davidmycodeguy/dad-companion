//! The card's window: transparent, click-through and always on top, shown beside the game's
//! tooltip without ever taking focus from the game.
//!
//! Showing a card is a round trip: Rust sends the card to the window's page, the page renders it
//! and reports its size, and Rust then places the window beside the tooltip and shows it. That way
//! the window is never shown with a stale card or at a wrong size.


use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindowBuilder};

use market::card::HoverCard;
/// A screen region in physical pixels: (left, top, right, bottom).
pub type Region = (i32, i32, i32, i32);

pub const CARD_WINDOW: &str = "card";
/// Space between the game's tooltip and the card, in physical pixels.
const GAP_PX: i32 = 12;
/// The window's size before the first card reports its own (CSS pixels).
const INITIAL_SIZE: (f64, f64) = (320.0, 480.0);
const CARD_EVENT: &str = "hover-card";

/// The latest card sent to the page (a sequence number) and, until the page has rendered it, where
/// it goes. One lock for both, so a show and a hide from different threads cannot interleave.
#[derive(Default)]
pub struct Overlay {
    state: Mutex<OverlayState>,
}

#[derive(Default)]
struct OverlayState {
    seq: u64,
    pending: Option<(u64, Region, (i32, i32))>,
}

impl Overlay {
    /// Starts the next card: shown at `place` once rendered, or hidden when None.
    fn next(&self, place: Option<(Region, (i32, i32))>) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.seq += 1;
        let seq = state.seq;
        state.pending = place.map(|(tooltip, screen)| (seq, tooltip, screen));
        seq
    }

    /// Where card `seq` goes, if it is still the latest one waiting to be shown.
    fn rendered(&self, seq: u64) -> Option<(Region, (i32, i32))> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match state.pending {
            Some((pending, tooltip, screen)) if pending == seq => {
                state.pending = None;
                Some((tooltip, screen))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Serialize)]
struct CardMessage<'a> {
    seq: u64,
    card: Option<&'a HoverCard>,
}

/// Creates the (hidden) card window at startup, so showing a card later costs no window creation.
pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, CARD_WINDOW, WebviewUrl::App("index.html#/card".into()))
        .title("DaD Companion card")
        .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    window.set_ignore_cursor_events(true)?;
    if let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) {
        window.set_zoom(card_zoom(monitor.size().height, monitor.scale_factor()))?;
    }
    Ok(())
}

/// The game's text grows with the screen, so the card does too: sized for a 1080p screen, scaled by
/// the screen's height and a touch larger (the game's tooltip text is large), whatever Windows'
/// display scaling. On a 4K screen at 150% that is about 1.47.
pub fn card_zoom(screen_height: u32, scale_factor: f64) -> f64 {
    const BASE_HEIGHT: f64 = 1080.0;
    const LARGER: f64 = 1.1;
    let zoom = f64::from(screen_height) / BASE_HEIGHT * LARGER / scale_factor.max(0.5);
    zoom.clamp(0.75, 3.0)
}

/// Sends `card` to the card window, to be shown beside `tooltip` (physical pixels) on a screen of
/// `screen` size once the page has rendered it.
pub fn show<R: Runtime>(app: &AppHandle<R>, card: &HoverCard, tooltip: Region, screen: (i32, i32)) {
    let seq = app.state::<Overlay>().next(Some((tooltip, screen)));
    let _ = app.emit_to(CARD_WINDOW, CARD_EVENT, CardMessage { seq, card: Some(card) });
}

/// Hides the card (and forgets one waiting to be shown).
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    let seq = app.state::<Overlay>().next(None);
    if let Some(window) = app.get_webview_window(CARD_WINDOW) {
        let _ = window.hide();
    }
    let _ = app.emit_to(CARD_WINDOW, CARD_EVENT, CardMessage { seq, card: None });
}

/// The page rendered card `seq` at `width` x `height` physical pixels (it knows its zoom and the
/// display scaling): place and show the window, unless a newer card (or a hide) came since.
#[tauri::command]
pub fn card_rendered<R: Runtime>(app: AppHandle<R>, seq: u64, width: f64, height: f64) -> Result<(), String> {
    let Some((tooltip, screen)) = app.state::<Overlay>().rendered(seq) else {
        return Ok(());
    };
    let window = app.get_webview_window(CARD_WINDOW).ok_or("card window missing")?;
    let size = (width.ceil() as i32, height.ceil() as i32);
    let (x, y) = place(size, tooltip, screen);
    window.set_size(PhysicalSize::new(size.0.max(1) as u32, size.1.max(1) as u32)).map_err(|e| e.to_string())?;
    window.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
    show_without_focus(&window);
    Ok(())
}

/// Where a card of `size` goes beside `tooltip` (left, top, right, bottom): to its left when it
/// fits, else to its right; kept on the screen.
pub fn place(size: (i32, i32), tooltip: Region, screen: (i32, i32)) -> (i32, i32) {
    let (width, height) = size;
    let (left, top, right, _) = tooltip;
    let mut x = left - GAP_PX - width;
    if x < 0 {
        x = right + GAP_PX;
    }
    let x = x.clamp(0, (screen.0 - width).max(0));
    let y = top.clamp(0, (screen.1 - height).max(0));
    (x, y)
}

/// Shows the window without activating it, so the game keeps keyboard and mouse focus, and puts it
/// on top of other always-on-top windows: a borderless game can itself sit in that band, and a
/// window keeps its old place in it when it is shown again.
#[cfg(windows)]
fn show_without_focus<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE,
    };
    match window.hwnd() {
        // SAFETY: the handle belongs to a live window of this process; both calls only read it.
        Ok(hwnd) => unsafe {
            ShowWindow(hwnd.0 as _, SW_SHOWNOACTIVATE);
            SetWindowPos(hwnd.0 as _, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        },
        Err(_) => {
            let _ = window.show();
        }
    }
}

#[cfg(not(windows))]
fn show_without_focus<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let _ = window.show();
}

#[cfg(test)]
mod tests {
    use super::{place, Overlay};

    #[test]
    fn only_the_latest_card_is_placed_once_rendered() {
        let overlay = Overlay::default();
        let tooltip = (2000, 300, 2600, 1300);
        let first = overlay.next(Some((tooltip, SCREEN)));
        let second = overlay.next(Some((tooltip, SCREEN)));
        assert_eq!(overlay.rendered(first), None); // a newer card came since
        assert_eq!(overlay.rendered(second), Some((tooltip, SCREEN)));
        assert_eq!(overlay.rendered(second), None); // placed once
        let third = overlay.next(Some((tooltip, SCREEN)));
        overlay.next(None); // hidden before it rendered
        assert_eq!(overlay.rendered(third), None);
    }

    const SCREEN: (i32, i32) = (3840, 2160);

    #[test]
    fn the_card_goes_left_of_the_tooltip_when_it_fits() {
        assert_eq!(place((450, 700), (2000, 300, 2600, 1300), SCREEN), (2000 - 12 - 450, 300));
    }

    #[test]
    fn otherwise_it_goes_right_of_it() {
        assert_eq!(place((450, 700), (200, 300, 800, 1300), SCREEN), (812, 300));
    }

    #[test]
    fn the_card_grows_with_the_screen_not_with_windows_scaling() {
        assert!((super::card_zoom(2160, 1.5) - 2160.0 / 1080.0 * 1.1 / 1.5).abs() < 1e-9);
        assert!((super::card_zoom(1080, 1.0) - 1.1).abs() < 1e-9);
        assert_eq!(super::card_zoom(720, 3.0), 0.75); // kept readable
    }

    #[test]
    fn it_stays_on_the_screen() {
        assert_eq!(place((450, 700), (100, 1800, 3500, 2100), SCREEN), (3390, 1460));
        assert_eq!(place((450, 700), (2000, -50, 2600, 900), SCREEN).1, 0);
    }
}
