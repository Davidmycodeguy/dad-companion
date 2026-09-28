//! Small composite macros built from the raw input primitives: click a point, click a stash tab.
//! Ports the click sequence inlined in `macros.py`'s `click_stash_tab`.

use std::time::Duration;

use crate::layout::{tab_index_for_stash_type, ScreenLayout};
use crate::sink::InputSink;
use crate::{Cancel, Error, Point};

const PRE_CLICK_DELAY: Duration = Duration::from_millis(50);
const CLICK_HOLD_DELAY: Duration = Duration::from_millis(30);
const POST_CLICK_DELAY: Duration = Duration::from_millis(150);

/// Moves to `point` and clicks it once: move, settle, press, hold, release, settle. Ports the
/// move/sleep/down/sleep/up/sleep sequence inlined in `click_stash_tab`.
pub fn click_point(sink: &mut dyn InputSink, cancel: &Cancel, point: Point) -> Result<(), Error> {
    cancel.check()?;
    sink.move_to(point.x, point.y);
    cancel.sleep(PRE_CLICK_DELAY)?;
    sink.mouse_down();
    cancel.sleep(CLICK_HOLD_DELAY)?;
    sink.mouse_up();
    cancel.sleep(POST_CLICK_DELAY)
}

/// Clicks the stash tab selector for `stash_type` under `mapping`. Returns `Ok(false)` without
/// clicking anything when `stash_type` has no tab (e.g. BAG, EQUIPMENT) or the mapped index is out
/// of range for `layout`'s tab positions. Ports `click_stash_tab`.
pub fn click_stash_tab(sink: &mut dyn InputSink, cancel: &Cancel, layout: &ScreenLayout, mapping: &[i32], stash_type: i32) -> Result<bool, Error> {
    let Some(tab_index) = tab_index_for_stash_type(mapping, stash_type) else {
        return Ok(false);
    };
    cancel.check()?;
    let positions = layout.stash_tab_positions();
    let Some(&point) = positions.get(tab_index) else {
        return Ok(false);
    };
    click_point(sink, cancel, point)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::InputEvent;
    use crate::DEFAULT_STASH_TAB_MAPPING;

    #[derive(Default)]
    struct RecordingSink(Vec<InputEvent>);

    impl InputSink for RecordingSink {
        fn move_to(&mut self, x: i32, y: i32) {
            self.0.push(InputEvent::MoveTo { x, y });
        }
        fn mouse_down(&mut self) {
            self.0.push(InputEvent::MouseDown);
        }
        fn mouse_up(&mut self) {
            self.0.push(InputEvent::MouseUp);
        }
        fn key_event(&mut self, vk: u16, key_up: bool) {
            self.0.push(InputEvent::Key { vk, key_up });
        }
        fn cursor_position(&self) -> (i32, i32) {
            (0, 0)
        }
    }

    #[test]
    fn click_point_moves_then_clicks_once() {
        let mut sink = RecordingSink::default();
        click_point(&mut sink, &Cancel::new(), Point::new(10, 20)).unwrap();
        assert_eq!(sink.0, vec![InputEvent::MoveTo { x: 10, y: 20 }, InputEvent::MouseDown, InputEvent::MouseUp]);
    }

    #[test]
    fn click_point_sends_nothing_once_cancelled() {
        let mut sink = RecordingSink::default();
        let cancel = Cancel::new();
        cancel.cancel();
        let err = click_point(&mut sink, &cancel, Point::new(10, 20)).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
        assert!(sink.0.is_empty());
    }

    #[test]
    fn click_stash_tab_reports_false_for_unmapped_stash_type() {
        let mut sink = RecordingSink::default();
        let layout = crate::positions_for_resolution(crate::BASE_RESOLUTION);
        let clicked = click_stash_tab(&mut sink, &Cancel::new(), &layout, &DEFAULT_STASH_TAB_MAPPING, 2).unwrap();
        assert!(!clicked);
        assert!(sink.0.is_empty());
    }

    #[test]
    fn click_stash_tab_clicks_the_mapped_tab_position() {
        let mut sink = RecordingSink::default();
        let layout = crate::positions_for_resolution(crate::BASE_RESOLUTION);
        // DEFAULT_STASH_TAB_MAPPING[0] == 4 (Storage) -> tab index 0 -> stash_tab_origin itself.
        let clicked = click_stash_tab(&mut sink, &Cancel::new(), &layout, &DEFAULT_STASH_TAB_MAPPING, 4).unwrap();
        assert!(clicked);
        let expected = layout.stash_tab_positions()[0];
        assert_eq!(sink.0[0], InputEvent::MoveTo { x: expected.x, y: expected.y });
    }
}
