//! Hotkey parsing/canonicalizing/display (pure) and the Windows `RegisterHotKey` backend.
//! `hotkeys.py` has no dedicated Python test file of its own to port 1:1 — these are original
//! tests written for this port, covering the same behaviour the Python source implements.

use input::hotkeys::{canonicalize_hotkey, format_hotkey_display, parse_hotkey, HotkeyError, HotkeyManager};

#[test]
fn modifier_aliases_collapse_to_one_canonical_form() {
    assert_eq!(canonicalize_hotkey("control+f1").unwrap(), "ctrl+f1");
    assert_eq!(canonicalize_hotkey("Super+a").unwrap(), "win+a");
    assert_eq!(canonicalize_hotkey("meta+a").unwrap(), "win+a");
    assert_eq!(canonicalize_hotkey("cmd+a").unwrap(), "win+a");
    assert_eq!(canonicalize_hotkey("windows+a").unwrap(), "win+a");
}

#[test]
fn modifiers_are_reordered_to_ctrl_alt_shift_win_regardless_of_input_order() {
    assert_eq!(canonicalize_hotkey("shift+win+alt+ctrl+f11").unwrap(), "ctrl+alt+shift+win+f11");
    assert_eq!(canonicalize_hotkey("alt+ctrl+f12").unwrap(), "ctrl+alt+f12");
}

#[test]
fn duplicate_modifiers_and_hyphen_separators_are_tolerated() {
    assert_eq!(canonicalize_hotkey("ctrl-ctrl-f11").unwrap(), "ctrl+f11");
    assert_eq!(canonicalize_hotkey("ctrl-shift-f12").unwrap(), "ctrl+shift+f12");
}

#[test]
fn key_aliases_collapse_to_their_canonical_name() {
    assert_eq!(canonicalize_hotkey("escape").unwrap(), "esc");
    assert_eq!(canonicalize_hotkey("return").unwrap(), "enter");
    assert_eq!(canonicalize_hotkey("spacebar").unwrap(), "space");
    assert_eq!(canonicalize_hotkey("pgup").unwrap(), "pageup");
    assert_eq!(canonicalize_hotkey("pgdn").unwrap(), "pagedown");
    assert_eq!(canonicalize_hotkey("del").unwrap(), "delete");
    assert_eq!(canonicalize_hotkey("ins").unwrap(), "insert");
    assert_eq!(canonicalize_hotkey("bksp").unwrap(), "backspace");
    assert_eq!(canonicalize_hotkey("caps").unwrap(), "capslock");
}

#[test]
fn single_letters_and_digits_resolve_to_their_ascii_virtual_key() {
    let a = parse_hotkey("a").unwrap();
    assert_eq!(a.canonical, "a");
    assert_eq!(a.vk_code, b'A' as u16);
    let nine = parse_hotkey("9").unwrap();
    assert_eq!(nine.vk_code, b'9' as u16);
}

#[test]
fn function_keys_resolve_across_the_full_f1_to_f24_range() {
    assert_eq!(parse_hotkey("f1").unwrap().vk_code, 0x70);
    assert_eq!(parse_hotkey("f11").unwrap().vk_code, 0x7A);
    assert_eq!(parse_hotkey("f24").unwrap().vk_code, 0x87);
    assert!(parse_hotkey("f25").is_err());
    assert!(parse_hotkey("f0").is_err());
}

#[test]
fn numpad_digits_and_operators_resolve_to_their_virtual_keys() {
    assert_eq!(parse_hotkey("numpad0").unwrap().vk_code, 0x60);
    assert_eq!(parse_hotkey("numpad9").unwrap().vk_code, 0x69);
    assert_eq!(parse_hotkey("multiply").unwrap().vk_code, 0x6A);
    assert_eq!(parse_hotkey("add").unwrap().vk_code, 0x6B);
    assert_eq!(parse_hotkey("subtract").unwrap().vk_code, 0x6D);
    assert_eq!(parse_hotkey("decimal").unwrap().vk_code, 0x6E);
    assert_eq!(parse_hotkey("divide").unwrap().vk_code, 0x6F);
}

#[test]
fn navigation_and_media_keys_resolve_to_their_special_virtual_keys() {
    assert_eq!(parse_hotkey("home").unwrap().vk_code, 0x24);
    assert_eq!(parse_hotkey("end").unwrap().vk_code, 0x23);
    assert_eq!(parse_hotkey("left").unwrap().vk_code, 0x25);
    assert_eq!(parse_hotkey("volumemute").unwrap().vk_code, 0xAD);
    // The literal "+" character can only ever reach the parser as a *separator*, never as a
    // token — `"+".split('+')` yields no non-empty tokens, in Rust just as in the Python
    // reference — so the "+" key is reachable only through its word alias, "plus".
    assert_eq!(parse_hotkey("plus").unwrap().vk_code, 0xBB);
    assert_eq!(parse_hotkey("ctrl+plus").unwrap().vk_code, 0xBB);
    assert!(parse_hotkey("+").is_err());
}

#[test]
fn empty_or_modifier_only_hotkeys_are_rejected() {
    assert!(matches!(parse_hotkey(""), Err(HotkeyError::Parse(_))));
    assert!(matches!(parse_hotkey("   "), Err(HotkeyError::Parse(_))));
    assert!(matches!(parse_hotkey("ctrl+shift"), Err(HotkeyError::Parse(_))));
}

#[test]
fn two_non_modifier_keys_are_rejected() {
    assert!(matches!(parse_hotkey("a+b"), Err(HotkeyError::Parse(_))));
}

#[test]
fn unsupported_keys_are_rejected() {
    assert!(matches!(parse_hotkey("banana"), Err(HotkeyError::Parse(_))));
}

#[test]
fn modifier_mask_combines_every_held_modifier() {
    let parsed = parse_hotkey("ctrl+alt+shift+win+z").unwrap();
    assert_eq!(parsed.modifier_mask, 0x0001 | 0x0002 | 0x0004 | 0x0008);
}

#[test]
fn display_uses_friendly_names_and_overrides() {
    assert_eq!(format_hotkey_display(Some("ctrl+f11"), ""), "Ctrl + F11");
    assert_eq!(format_hotkey_display(Some("ctrl+pageup"), ""), "Ctrl + Page Up");
    assert_eq!(format_hotkey_display(Some("ctrl+a"), ""), "Ctrl + A");
    assert_eq!(format_hotkey_display(Some("win+enter"), ""), "Win + Enter");
}

#[test]
fn display_falls_back_when_raw_is_empty_or_unparsable() {
    assert_eq!(format_hotkey_display(None, "ctrl+f12"), "Ctrl + F12");
    assert_eq!(format_hotkey_display(Some(""), "ctrl+f12"), "Ctrl + F12");
    // Whitespace-only raw is a non-empty string in the source language too, but strips to empty:
    // no fallback is consulted, matching the Python reference's `(raw or fallback or "").strip()`.
    assert_eq!(format_hotkey_display(Some("   "), "ctrl+f12"), "");
    // Unparsable raw with a good fallback: the fallback wins.
    assert_eq!(format_hotkey_display(Some("not a hotkey"), "ctrl+f9"), "Ctrl + F9");
    // Both unparsable: displayed verbatim.
    assert_eq!(format_hotkey_display(Some("???"), "???"), "???");
    assert_eq!(format_hotkey_display(None, ""), "");
}

#[test]
fn canonicalize_hotkey_matches_parse_hotkeys_canonical_field() {
    assert_eq!(canonicalize_hotkey("Ctrl+F11").unwrap(), parse_hotkey("ctrl+f11").unwrap().canonical);
}

// --- Windows RegisterHotKey backend -----------------------------------------------------------
//
// Each test below uses its own, unique, unlikely modifier+key combination — cargo runs tests in
// parallel within one process, and a global hotkey is registered per-*thread*, so two tests
// sharing one combination would race each other at the OS level, not just in this crate. Every
// combination is unregistered before the test ends (or via `HotkeyManager::drop`), so none of
// this can collide with a hotkey the owner may already have bound elsewhere.

#[test]
fn register_and_unregister_a_real_hotkey() {
    let manager = HotkeyManager::new().expect("hotkey listener thread should start");
    let canonical = manager.register("test-a", "ctrl+alt+shift+win+f24", || {}).expect("registration should succeed");
    assert_eq!(canonical, "ctrl+alt+shift+win+f24");
    manager.unregister("test-a").expect("unregistration should succeed");
}

#[test]
fn re_registering_the_same_label_with_a_new_hotkey_moves_the_binding() {
    let manager = HotkeyManager::new().expect("hotkey listener thread should start");
    manager.register("test-b", "ctrl+alt+shift+win+f23", || {}).expect("first registration should succeed");
    let canonical =
        manager.register("test-b", "ctrl+alt+shift+win+f22", || {}).expect("re-registration should succeed");
    assert_eq!(canonical, "ctrl+alt+shift+win+f22");
    manager.unregister("test-b").expect("cleanup should succeed");
}

#[test]
fn registering_the_same_hotkey_under_a_different_label_conflicts() {
    let manager = HotkeyManager::new().expect("hotkey listener thread should start");
    manager.register("owner", "ctrl+alt+shift+win+f21", || {}).expect("first registration should succeed");
    let err = manager.register("other", "ctrl+alt+shift+win+f21", || {}).unwrap_err();
    assert!(matches!(err, HotkeyError::Conflict(_)), "expected a conflict error, got {err:?}");
    manager.unregister("owner").expect("cleanup should succeed");
}

#[test]
fn dropping_the_manager_unregisters_everything() {
    {
        let manager = HotkeyManager::new().expect("hotkey listener thread should start");
        manager.register("temp", "ctrl+alt+shift+win+f20", || {}).expect("registration should succeed");
        // `manager` drops here, which must unregister the binding above.
    }
    // If Drop failed to unregister, this second manager's registration of the same combination
    // would still succeed anyway (RegisterHotKey allows the same process to re-register), so the
    // meaningful assertion is just that registering it again from a fresh manager doesn't panic or
    // hang — the real regression this guards is a stuck listener thread on drop.
    let manager = HotkeyManager::new().expect("hotkey listener thread should start");
    manager.register("temp2", "ctrl+alt+shift+win+f20", || {}).expect("registration after drop should succeed");
    manager.unregister("temp2").expect("cleanup should succeed");
}
