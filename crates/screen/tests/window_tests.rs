//! Window/cursor helper tests that don't depend on the game actually running.

use screen::{cursor_position, game_has_focus, make_process_dpi_aware, screen_size};

#[test]
fn made_up_exe_name_never_has_focus() {
    assert!(!game_has_focus("this-process-definitely-does-not-exist.exe"));
}

#[test]
fn primary_monitor_reports_a_size() {
    let _ = make_process_dpi_aware();
    let (width, height) = screen_size();
    assert!(width > 0 && height > 0, "got {width}x{height}");
}

#[test]
#[ignore = "only on the 4K dev machine: cargo test -p screen -- --ignored"]
fn primary_monitor_is_the_known_4k_display() {
    // The dev machine's primary display is 3840x2160; asserting the exact value (rather than just
    // "> 0") also proves DESKTOPHORZRES/VERTRES bypassed DPI virtualization.
    let _ = make_process_dpi_aware();
    assert_eq!(screen_size(), (3840, 2160));
}

#[test]
fn cursor_position_reports_a_location() {
    let _ = make_process_dpi_aware();
    assert!(cursor_position().is_some(), "a live desktop session always has a cursor position");
}
