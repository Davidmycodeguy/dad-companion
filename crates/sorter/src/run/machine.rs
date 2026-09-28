//! What a run needs from the machine besides the mouse: screen captures, the cursor, and the game
//! window. [`Desktop`] is the real one; tests substitute a fake that simulates the game.

use input::InputSink;
use screen::{Frame, Region};

use super::report::StopReason;

/// The screen and the game window, as a run sees them.
pub trait Machine: Send + Sync {
    /// Captures `region` (physical pixels, right/bottom exclusive).
    fn grab(&self, region: Region) -> Result<Frame, String>;
    /// The cursor's position in physical pixels, if Windows can say.
    fn cursor(&self) -> Option<(i32, i32)>;
    /// Whether the game (or an overlay drawn over it) has focus.
    fn game_focused(&self) -> bool;
    /// Finds the game window and brings it to the front. `sink` sends the Alt tap Windows requires
    /// before it lets another program take the foreground.
    fn bring_game_forward(&self, sink: &mut dyn InputSink) -> Result<(), StopReason>;
}

/// The real screen and game window.
#[derive(Debug, Clone, Copy, Default)]
pub struct Desktop;

impl Machine for Desktop {
    fn grab(&self, region: Region) -> Result<Frame, String> {
        screen::grab(region).map_err(|err| err.to_string())
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        screen::cursor_position()
    }

    fn game_focused(&self) -> bool {
        screen::game_has_focus(input::GAME_PROCESS_EXE)
    }

    fn bring_game_forward(&self, sink: &mut dyn InputSink) -> Result<(), StopReason> {
        let window = input::find_game_window().ok_or(StopReason::GameNotFound)?;
        // Windows may refuse the switch and still leave the game in front (it already was), so a
        // refusal is not an error by itself: the run waits for focus next and decides from that.
        let _ = input::bring_to_front(sink, window);
        Ok(())
    }
}
