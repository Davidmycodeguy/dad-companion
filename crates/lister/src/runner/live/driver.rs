//! Real Windows input for the runners: DnDTools' `MacrosInputDriver` (`marketplace_input.py`).

use std::thread::sleep;
use std::time::Duration;

use input::{InputSink, Point, WindowsInput, VK_CONTROL};

use crate::runner::{GameInput, InputError, ScreenPoint};

/// How long the button stays down in a click.
const CLICK_HOLD: Duration = Duration::from_millis(120);
/// A click arrives with a move to this many pixels up-left of the target first.
const APPROACH_OFFSET_PX: i32 = 5;
const APPROACH: Duration = Duration::from_millis(120);
/// Lets the game register the hover before the button goes down.
const HOVER_SETTLE: Duration = Duration::from_millis(150);
/// A cursor this close to the target is already hovering it (e.g. paging): no approach move.
const SAME_SPOT_PX: u32 = 2;
const REPEAT_CLICK_SETTLE: Duration = Duration::from_millis(50);
const KEY_GAP: Duration = Duration::from_millis(30);
const VK_BACK: u16 = 0x08;
const VK_A: u16 = 0x41;
const VK_DIGIT_0: u16 = 0x30;
/// Drag timing: hold, glide in steps, hold, release — the timing used for the first merchant sales
/// in game (2026-09-27); shorter timings were never tried.
const DRAG_HOLD: Duration = Duration::from_millis(250);
const DRAG_STEPS: i32 = 12;
const DRAG_STEP: Duration = Duration::from_millis(30);
const ESC_SCAN_CODE: u16 = 0x01;
const KEY_HOLD: Duration = Duration::from_millis(80);

/// Sends real mouse and keyboard input with the timings verified in game. The waits inside one
/// click, drag or key press are never cut short: a click is always sent whole (the runners check
/// for a cancel between steps, not in the middle of one).
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveInput;

/// Arrives at `point` with a small approach move and lets the hover register: the game ignores
/// instant clicks (verified in game).
fn approach(sink: &mut WindowsInput, (x, y): ScreenPoint) {
    sink.move_to(x - APPROACH_OFFSET_PX, y - APPROACH_OFFSET_PX);
    sleep(APPROACH);
    sink.move_to(x, y);
    sleep(HOVER_SETTLE);
}

fn tap(sink: &mut WindowsInput, vk: u16) {
    input::send_key(sink, vk, false);
    sleep(KEY_GAP);
    input::send_key(sink, vk, true);
    sleep(KEY_GAP);
}

impl GameInput for LiveInput {
    fn click(&self, point: ScreenPoint) -> Result<(), InputError> {
        let mut sink = WindowsInput;
        let (x, y) = self.position()?;
        if x.abs_diff(point.0) <= SAME_SPOT_PX && y.abs_diff(point.1) <= SAME_SPOT_PX {
            sleep(REPEAT_CLICK_SETTLE);
        } else {
            approach(&mut sink, point);
        }
        sink.mouse_down();
        sleep(CLICK_HOLD);
        sink.mouse_up();
        Ok(())
    }

    fn move_to(&self, (x, y): ScreenPoint) -> Result<(), InputError> {
        WindowsInput.move_to(x, y);
        Ok(())
    }

    /// Picks the item up at `from`, glides to `to` and drops it there (verified in game).
    fn drag(&self, from: ScreenPoint, to: ScreenPoint) -> Result<(), InputError> {
        let mut sink = WindowsInput;
        approach(&mut sink, from);
        sink.mouse_down();
        sleep(DRAG_HOLD);
        for step in 1..=DRAG_STEPS {
            let along = |start: i32, end: i32| f64::from(start) + f64::from((end - start) * step) / f64::from(DRAG_STEPS);
            let point = Point::rounded(along(from.0, to.0), along(from.1, to.1));
            sink.move_to(point.x, point.y);
            sleep(DRAG_STEP);
        }
        sleep(DRAG_HOLD);
        sink.mouse_up();
        Ok(())
    }

    fn clear_and_type(&self, text: &str) -> Result<(), InputError> {
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(InputError::new("price must be digits only"));
        }
        let mut sink = WindowsInput;
        input::send_key(&mut sink, VK_CONTROL, false);
        tap(&mut sink, VK_A);
        input::send_key(&mut sink, VK_CONTROL, true);
        tap(&mut sink, VK_BACK);
        for digit in text.bytes() {
            tap(&mut sink, VK_DIGIT_0 + u16::from(digit - b'0'));
        }
        input::release_modifiers(&mut sink);
        Ok(())
    }

    /// Escape by hardware scan code: the game reads raw input and ignores a virtual-key Escape.
    fn press_escape(&self) -> Result<(), InputError> {
        for key_up in [false, true] {
            scan_key::send(ESC_SCAN_CODE, key_up);
            sleep(KEY_HOLD);
        }
        Ok(())
    }

    fn position(&self) -> Result<ScreenPoint, InputError> {
        screen::cursor_position().ok_or_else(|| InputError::new("couldn't read the mouse position"))
    }
}

/// A key press by hardware scan code. The input crate only sends virtual keys, so this one call is
/// declared here: the same `SendInput` it uses, with `KEYEVENTF_SCANCODE` set.
#[allow(dead_code)] // `#[repr(C)]` mirrors of Win32 structs: Windows reads the fields, Rust doesn't.
mod scan_key {
    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const KEYEVENTF_SCANCODE: u32 = 0x0008;

    /// Win32 `MOUSEINPUT`, only here so [`InputData`] has the size of `INPUT`'s union.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MouseInput {
        dx: i32,
        dy: i32,
        mouse_data: u32,
        flags: u32,
        time: u32,
        extra_info: usize,
    }

    /// Win32 `KEYBDINPUT`.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KeybdInput {
        vk: u16,
        scan: u16,
        flags: u32,
        time: u32,
        extra_info: usize,
    }

    #[repr(C)]
    union InputData {
        mouse: MouseInput,
        keyboard: KeybdInput,
    }

    /// Win32 `INPUT`.
    #[repr(C)]
    pub(super) struct Input {
        kind: u32,
        data: InputData,
    }

    #[link(name = "user32", kind = "raw-dylib")]
    extern "system" {
        fn SendInput(count: u32, inputs: *const Input, size: i32) -> u32;
    }

    /// Presses (`key_up = false`) or releases the key with hardware scan code `scan`.
    pub(super) fn send(scan: u16, key_up: bool) {
        let flags = KEYEVENTF_SCANCODE | if key_up { KEYEVENTF_KEYUP } else { 0 };
        let keyboard = KeybdInput { vk: 0, scan, flags, time: 0, extra_info: 0 };
        let input = Input { kind: INPUT_KEYBOARD, data: InputData { keyboard } };
        // SAFETY: `input` is one fully initialized `INPUT` (the `#[repr(C)]` mirror above, its union
        // sized by the largest member) on the stack, and `size` is its size; `SendInput` reads it
        // synchronously and keeps no pointer to it. The count of events accepted is ignored, as
        // DnDTools ignored it.
        unsafe {
            SendInput(1, &input, std::mem::size_of::<Input>() as i32);
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn input_has_the_size_windows_expects() {
            let expected = if cfg!(target_pointer_width = "64") { 40 } else { 28 };
            assert_eq!(std::mem::size_of::<super::Input>(), expected);
        }
    }
}
