//! Keyboard input: virtual-key send, the Alt-tap focus trick, and releasing stuck modifiers.
//!
//! Ports the keyboard half of `macros.py`: `send_key`, `send_alt_up`/`send_ctrl_up` (folded into
//! [`release_modifiers`], their only caller), and `tap_alt`.

use std::time::Duration;

use crate::sink::InputSink;

/// Alt. Ports `macros.py`'s `VK_MENU`.
pub const VK_MENU: u16 = 0x12;
/// Ctrl. Ports `macros.py`'s `VK_CONTROL`.
pub const VK_CONTROL: u16 = 0x11;

/// `tap_alt`'s Python default delay (`delay: float = 0.035`).
pub const DEFAULT_TAP_ALT_DELAY: Duration = Duration::from_millis(35);

/// Presses (`key_up = false`) or releases (`key_up = true`) the virtual key `vk`. Ports `send_key`.
pub fn send_key(sink: &mut dyn InputSink, vk: u16, key_up: bool) {
    sink.key_event(vk, key_up);
}

/// Presses Alt, holds it for `delay`, then releases it.
///
/// Windows refuses an unsolicited `SetForegroundWindow` from a background process unless the
/// calling thread just generated input — this is what makes
/// [`crate::window::bring_to_front`]'s foreground switch succeed, in addition to its original
/// purpose of clearing any Alt state left stuck by an interrupted macro. Uses a plain
/// (non-cancellable) sleep, exactly as the Python reference does. Ports `tap_alt`.
pub fn tap_alt(sink: &mut dyn InputSink, delay: Duration) {
    sink.key_event(VK_MENU, false);
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    sink.key_event(VK_MENU, true);
}

/// Releases Alt and Ctrl, in case either was left stuck down by an interrupted macro. Ports
/// `release_modifiers` (`send_alt_up` + `send_ctrl_up`).
pub fn release_modifiers(sink: &mut dyn InputSink) {
    sink.key_event(VK_MENU, true);
    sink.key_event(VK_CONTROL, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::InputEvent;

    #[derive(Default)]
    struct RecordingSink(Vec<InputEvent>);

    impl InputSink for RecordingSink {
        fn move_to(&mut self, _x: i32, _y: i32) {}
        fn mouse_down(&mut self) {}
        fn mouse_up(&mut self) {}
        fn key_event(&mut self, vk: u16, key_up: bool) {
            self.0.push(InputEvent::Key { vk, key_up });
        }
        fn cursor_position(&self) -> (i32, i32) {
            (0, 0)
        }
    }

    #[test]
    fn tap_alt_presses_then_releases() {
        let mut sink = RecordingSink::default();
        tap_alt(&mut sink, Duration::ZERO);
        assert_eq!(sink.0, vec![InputEvent::Key { vk: VK_MENU, key_up: false }, InputEvent::Key { vk: VK_MENU, key_up: true }]);
    }

    #[test]
    fn release_modifiers_releases_alt_and_ctrl() {
        let mut sink = RecordingSink::default();
        release_modifiers(&mut sink);
        assert_eq!(sink.0, vec![InputEvent::Key { vk: VK_MENU, key_up: true }, InputEvent::Key { vk: VK_CONTROL, key_up: true }]);
    }

    #[test]
    fn send_key_forwards_vk_and_flag() {
        let mut sink = RecordingSink::default();
        send_key(&mut sink, 0x41, false);
        assert_eq!(sink.0, vec![InputEvent::Key { vk: 0x41, key_up: false }]);
    }
}
