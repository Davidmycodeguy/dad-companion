//! Parsing, canonicalizing and formatting hotkey strings such as `"ctrl+shift+f11"`. Ports the
//! pure half of `hotkeys.py` (everything above `_WindowsHotkeyBackend`); the cross-platform
//! `keyboard`-package string format has no Rust counterpart since this backend only ever targets
//! Windows `RegisterHotKey`.

use super::HotkeyError;

/// A modifier key. `Ord`'s derive gives exactly `hotkeys.py`'s canonical ordering
/// (`_MODIFIER_ORDER = ("ctrl", "alt", "shift", "win")`) since that's this enum's declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

impl Modifier {
    fn canonical_name(self) -> &'static str {
        match self {
            Modifier::Ctrl => "ctrl",
            Modifier::Alt => "alt",
            Modifier::Shift => "shift",
            Modifier::Win => "win",
        }
    }

    /// `RegisterHotKey`'s `MOD_*` bit. Ports `_WINDOWS_MODIFIER_BITS`.
    fn win32_bit(self) -> u32 {
        match self {
            Modifier::Alt => 0x0001,
            Modifier::Ctrl => 0x0002,
            Modifier::Shift => 0x0004,
            Modifier::Win => 0x0008,
        }
    }

    /// Ports `_MODIFIER_ALIASES`.
    fn from_alias(token: &str) -> Option<Self> {
        Some(match token {
            "ctrl" | "control" => Modifier::Ctrl,
            "alt" => Modifier::Alt,
            "shift" => Modifier::Shift,
            "win" | "windows" | "super" | "meta" | "cmd" => Modifier::Win,
            _ => return None,
        })
    }
}

/// A parsed, canonicalized hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHotkey {
    /// Canonical text form, e.g. `"ctrl+alt+f11"` — modifiers in canonical order, joined with `+`.
    pub canonical: String,
    /// Modifiers in canonical order (`Ctrl, Alt, Shift, Win`).
    pub modifiers: Vec<Modifier>,
    /// The non-modifier key, alias-resolved (e.g. `"escape"` becomes `"esc"`).
    pub key: String,
    /// `RegisterHotKey`'s `fsModifiers` bitmask.
    pub modifier_mask: u32,
    /// `RegisterHotKey`'s virtual-key code.
    pub vk_code: u16,
}

/// Collapses an alias to its canonical key name. Ports `_KEY_ALIASES`.
fn key_alias(token: &str) -> String {
    match token {
        "escape" => "esc",
        "return" => "enter",
        "spacebar" => "space",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        "del" => "delete",
        "ins" => "insert",
        "bksp" => "backspace",
        "caps" => "capslock",
        "plus" => "+",
        other => other,
    }
    .to_string()
}

/// Parses `raw` (e.g. `"Ctrl+F11"`, `"ctrl-shift-f12"`; `+`/`-` both separate tokens, matching the
/// Python reference) into a [`ParsedHotkey`]. Ports `_parse_hotkey`.
pub fn parse_hotkey(raw: &str) -> Result<ParsedHotkey, HotkeyError> {
    let tokens: Vec<String> =
        raw.replace('-', "+").split('+').map(str::trim).filter(|s| !s.is_empty()).map(str::to_lowercase).collect();
    if tokens.is_empty() {
        return Err(HotkeyError::Parse("Hotkey cannot be empty".to_string()));
    }

    let mut modifiers = Vec::new();
    let mut key: Option<String> = None;
    for token in &tokens {
        if let Some(modifier) = Modifier::from_alias(token) {
            if !modifiers.contains(&modifier) {
                modifiers.push(modifier);
            }
            continue;
        }
        if key.is_some() {
            return Err(HotkeyError::Parse("Hotkey may only specify one non-modifier key".to_string()));
        }
        key = Some(key_alias(token));
    }
    let key = key.ok_or_else(|| HotkeyError::Parse("Hotkey must include a non-modifier key".to_string()))?;

    modifiers.sort();
    let vk_code = key_to_vk(&key)?;
    let modifier_mask = modifiers.iter().fold(0u32, |mask, m| mask | m.win32_bit());
    let canonical = if modifiers.is_empty() {
        key.clone()
    } else {
        let mut parts: Vec<&str> = modifiers.iter().map(|m| m.canonical_name()).collect();
        parts.push(&key);
        parts.join("+")
    };

    Ok(ParsedHotkey { canonical, modifiers, key, modifier_mask, vk_code })
}

/// The canonical text form of `raw`. Ports `canonicalize_hotkey`.
pub fn canonicalize_hotkey(raw: &str) -> Result<String, HotkeyError> {
    parse_hotkey(raw).map(|parsed| parsed.canonical)
}

/// Virtual-key codes not covered by A-Z/0-9, F1-F24, numpad digits or the numpad operators. Ports
/// `_SPECIAL_VK`.
const SPECIAL_VK: &[(&str, u16)] = &[
    ("backspace", 0x08),
    ("tab", 0x09),
    ("enter", 0x0D),
    ("capslock", 0x14),
    ("esc", 0x1B),
    ("space", 0x20),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("end", 0x23),
    ("home", 0x24),
    ("left", 0x25),
    ("up", 0x26),
    ("right", 0x27),
    ("down", 0x28),
    ("insert", 0x2D),
    ("delete", 0x2E),
    ("printscreen", 0x2C),
    ("scrolllock", 0x91),
    ("pause", 0x13),
    ("numlock", 0x90),
    ("apps", 0x5D),
    ("volumeup", 0xAF),
    ("volumedown", 0xAE),
    ("volumemute", 0xAD),
    ("mediaplaypause", 0xB3),
    ("mediastop", 0xB2),
    ("medianext", 0xB0),
    ("mediaprevious", 0xB1),
    ("+", 0xBB),
];

/// Ports `_key_to_vk`.
fn key_to_vk(key: &str) -> Result<u16, HotkeyError> {
    if key.len() == 1 {
        let upper = key.to_ascii_uppercase().into_bytes()[0];
        if upper.is_ascii_uppercase() || upper.is_ascii_digit() {
            return Ok(u16::from(upper));
        }
    }
    if let Some(rest) = key.strip_prefix('f') {
        if let Ok(index @ 1..=24) = rest.parse::<u32>() {
            return Ok((0x6F + index) as u16);
        }
    }
    if let Some(rest) = key.strip_prefix("numpad") {
        if let Ok(index @ 0..=9) = rest.parse::<u32>() {
            return Ok((0x60 + index) as u16);
        }
    }
    match key {
        "multiply" => return Ok(0x6A),
        "add" => return Ok(0x6B),
        "subtract" => return Ok(0x6D),
        "decimal" => return Ok(0x6E),
        "divide" => return Ok(0x6F),
        _ => {}
    }
    if let Some(&(_, vk)) = SPECIAL_VK.iter().find(|(name, _)| *name == key) {
        return Ok(vk);
    }
    Err(HotkeyError::Parse(format!("Unsupported hotkey key: {key}")))
}

/// Ports `_HOTKEY_DISPLAY_OVERRIDES`.
const DISPLAY_OVERRIDES: &[(&str, &str)] = &[
    ("pageup", "Page Up"),
    ("pagedown", "Page Down"),
    ("capslock", "Caps Lock"),
    ("numlock", "Num Lock"),
    ("scrolllock", "Scroll Lock"),
    ("printscreen", "Print Screen"),
    ("esc", "Esc"),
    ("escape", "Esc"),
    ("space", "Space"),
    ("tab", "Tab"),
    ("enter", "Enter"),
    ("win", "Win"),
    ("windows", "Win"),
];

/// A user-friendly label for `raw` (falling back to `fallback` if `raw` is empty, and to whichever
/// of the two parses if the other doesn't), e.g. `"Ctrl + Shift + F11"`. Ports
/// `format_hotkey_display`.
pub fn format_hotkey_display(raw: Option<&str>, fallback: &str) -> String {
    let raw = raw.unwrap_or("");
    let chosen = if raw.is_empty() { fallback } else { raw };
    let candidate = chosen.trim();
    if candidate.is_empty() {
        return String::new();
    }

    let canonical = parse_hotkey(candidate).map(|p| p.canonical).unwrap_or_else(|_| {
        let fallback_trimmed = fallback.trim();
        let source = if fallback_trimmed.is_empty() { candidate } else { fallback_trimmed };
        parse_hotkey(source).map(|p| p.canonical).unwrap_or_else(|_| source.to_string())
    });

    let tokens: Vec<&str> = canonical.split('+').filter(|s| !s.is_empty()).collect();
    if tokens.is_empty() {
        return canonical.replace('+', " + ");
    }

    tokens.into_iter().map(display_token).collect::<Vec<_>>().join(" + ")
}

fn display_token(token: &str) -> String {
    let lower = token.to_lowercase();
    if let Some(&(_, display)) = DISPLAY_OVERRIDES.iter().find(|(name, _)| *name == lower) {
        return display.to_string();
    }
    if token.chars().count() == 1 {
        return token.to_uppercase();
    }
    if let Some(rest) = lower.strip_prefix('f') {
        if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
            return lower.to_uppercase();
        }
    }
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
