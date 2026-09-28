//! Turns the OCR'd lines of a game item tooltip into an item id and its stats.
//!
//! Tooltips list the title, then base stats as "Name value" and random rolls as "+value Name",
//! then Required Class / Slot Type / ... / "Rarity: X" and the flavour text. Values shown with %
//! are stored ×10 in the game data (Armor Penetration 30% = 300), plain ones as shown.
//!
//! Port of DnDTools' `tooltip_parser.py`.

mod difflib;

pub use difflib::{get_close_matches, ratio};

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use game_data::{ItemCatalog, Rarity};

use crate::OcrLine;

/// Fuzzy item-name match cutoff: loose enough for OCR slips like "Occu1tist Rohe".
const NAME_CUTOFF: f64 = 0.85;
/// Fuzzy stat-name match cutoff: names get cut short at the tooltip's crop edge.
const STAT_CUTOFF: f64 = 0.8;
/// Fuzzy match cutoff for telling which flat-or-percent stat family (`FLAT_OR_PERCENT`) a roll's
/// name belongs to, before picking its flat or percent id.
const FAMILY_CUTOFF: f64 = 0.9;
/// Values shown with a "%" are stored ×10 in the game data (30% = 300).
const PERCENT_SCALE: i64 = 10;

/// Base stats ("Name value"), keyed by the name with spaces and case removed.
const BASE_NAMES: [(&str, &str); 45] = [
    ("weapondamage", "PhysicalWeaponDamage"),
    ("magicweapondamage", "MagicalWeaponDamage"),
    ("magicalweapondamage", "MagicalWeaponDamage"),
    ("armorrating", "ArmorRating"),
    ("magicresistance", "MagicRegistance"),
    ("movespeed", "MoveSpeed"),
    ("armorpenetration", "ArmorPenetration"),
    ("magicpenetration", "MagicPenetration"),
    ("projectiledamagereduction", "ProjectileReductionMod"),
    ("headshotdamagereduction", "HeadshotReductionMod"),
    ("actionspeed", "ActionSpeed"),
    ("luck", "Luck"),
    ("magicaldamage", "MagicalDamage"),
    ("strength", "Strength"),
    ("vigor", "Vigor"),
    ("agility", "Agility"),
    ("dexterity", "Dexterity"),
    ("will", "Will"),
    ("knowledge", "Knowledge"),
    ("resourcefulness", "Resourcefulness"),
    ("physicalpower", "PhysicalPower"),
    ("magicalpower", "MagicalPower"),
    ("magicalhealing", "MagicalHealing"),
    ("regularinteractionspeed", "RegularInteractionSpeed"),
    ("magicalinteractionspeed", "MagicalInteractionSpeed"),
    ("spellcastingspeed", "SpellCastingSpeed"),
    ("buffdurationbonus", "BuffDurationBonus"),
    ("debuffdurationbonus", "DebuffDurationBonus"),
    ("cooldownreduction", "CooldownReductionBonus"),
    ("cooldownreductionbonus", "CooldownReductionBonus"),
    ("magicaldamagebonus", "MagicalDamageBonus"),
    ("magicaldamagereduction", "MagicalDamageReduction"),
    ("physicaldamagereduction", "PhysicalDamageReduction"),
    ("truemagicaldamage", "MagicalDamageTrue"),
    ("truephysicaldamage", "PhysicalDamageTrue"),
    ("undeaddamagebonus", "UndeadDamageMod"),
    ("undeaddamagereduction", "UndeadReductionMod"),
    ("demondamagebonus", "DemonDamageMod"),
    ("demondamagereduction", "DemonReductionMod"),
    ("headshotdamage", "HeadshotDamageMod"),
    ("headshotdamagebonus", "HeadshotDamageMod"),
    ("maxhealth", "MaxHealthAdd"),
    ("maxhealthbonus", "MaxHealthBonus"),
    ("memorycapacity", "MemoryCapacityAdd"),
    ("movespeedbonus", "MoveSpeedBonus"),
];

/// Random rolls ("+value Name"), keyed the same way. A few names mean a flat or a % stat
/// depending on whether a "%" was read (see `FLAT_OR_PERCENT`).
const ROLL_NAMES: [(&str, &str); 48] = [
    ("armorrating", "ArmorRatingAdd"),
    ("weapondamage", "PhysicalWeaponDamageAdd"),
    ("additionalweapondamage", "PhysicalWeaponDamageAdd"),
    ("physicaldamage", "PhysicalDamageAdd"),
    ("additionalphysicaldamage", "PhysicalDamageAdd"),
    ("physicaldamagebonus", "PhysicalDamageBonus"),
    ("truephysicaldamage", "PhysicalDamageTrue"),
    ("physicaldamagereduction", "PhysicalDamageReduction"),
    ("physicalpower", "PhysicalPower"),
    ("physicalhealing", "PhysicalHealing"),
    ("magicaldamage", "MagicalDamageAdd"),
    ("additionalmagicaldamage", "MagicalDamageAdd"),
    ("magicaldamagebonus", "MagicalDamageBonus"),
    ("truemagicaldamage", "MagicalDamageTrue"),
    ("magicaldamagereduction", "MagicalDamageReduction"),
    ("magicalpower", "MagicalPower"),
    ("magicalhealing", "MagicalHealing"),
    ("magicalinteractionspeed", "MagicalInteractionSpeed"),
    ("regularinteractionspeed", "RegularInteractionSpeed"),
    ("spellcastingspeed", "SpellCastingSpeed"),
    ("actionspeed", "ActionSpeed"),
    ("armorpenetration", "ArmorPenetration"),
    ("magicpenetration", "MagicPenetration"),
    ("magicresistance", "MagicRegistance"),
    ("headshotdamage", "HeadshotDamageMod"),
    ("headshotdamagebonus", "HeadshotDamageMod"),
    ("headshotdamagereduction", "HeadshotReductionMod"),
    ("projectiledamagereduction", "ProjectileReductionMod"),
    ("buffdurationbonus", "BuffDurationBonus"),
    ("debuffdurationbonus", "DebuffDurationBonus"),
    ("cooldownreduction", "CooldownReductionBonus"),
    ("cooldownreductionbonus", "CooldownReductionBonus"),
    ("undeaddamagebonus", "UndeadDamageMod"),
    ("undeaddamagereduction", "UndeadReductionMod"),
    ("demondamagebonus", "DemonDamageMod"),
    ("demondamagereduction", "DemonReductionMod"),
    ("luck", "Luck"),
    ("allattributes", "AllAttributes"),
    ("strength", "Strength"),
    ("vigor", "Vigor"),
    ("agility", "Agility"),
    ("dexterity", "Dexterity"),
    ("will", "Will"),
    ("knowledge", "Knowledge"),
    ("resourcefulness", "Resourcefulness"),
    ("memorycapacitybonus", "MemoryCapacityBonus"),
    ("maxhealthbonus", "MaxHealthBonus"),
    ("movespeedbonus", "MoveSpeedBonus"),
];

/// Roll names that mean a flat stat id or a percent stat id depending on the roll's "%".
const FLAT_OR_PERCENT: [(&str, (&str, &str)); 3] = [
    ("maxhealth", ("MaxHealthAdd", "MaxHealthBonus")),
    ("movespeed", ("MoveSpeedAdd", "MoveSpeedBonus")),
    ("memorycapacity", ("MemoryCapacityAdd", "MemoryCapacityBonus")),
];

/// `FLAT_OR_PERCENT`'s own keys, for fuzzy-matching a roll's name to the right family before
/// picking its flat/percent id (Python re-derives `{k: k for k in FLAT_OR_PERCENT}` on the fly;
/// since the key set is fixed, this is the same table written out once).
const FLAT_OR_PERCENT_FAMILIES: [(&str, &str); 3] = [
    ("maxhealth", "maxhealth"),
    ("movespeed", "movespeed"),
    ("memorycapacity", "memorycapacity"),
];

/// The first four letters of a normalized rarity word, to its canonical name.
const RARITY_KEYS: [(&str, &str); 8] = [
    ("poor", "Poor"),
    ("comm", "Common"),
    ("unco", "Uncommon"),
    ("rare", "Rare"),
    ("epic", "Epic"),
    ("lege", "Legendary"),
    ("uniq", "Unique"),
    ("arti", "Artifact"),
];

/// A run of digits, tolerating the letters OCR most often confuses with them (see `value`).
const NUMBER_PATTERN: &str = r"[\dIl|Oo]+(?:[.,]\d+)?";

/// "+value[%] Name...": a random roll.
static ROLL_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^\+\s*(?P<num>{NUMBER_PATTERN})\s*(?P<pct>%)?\s*(?P<name>[A-Za-z].*)$"))
        .expect("ROLL_LINE pattern is valid")
});
/// "Name value[%|f]": a base stat ("f" is how OCR sometimes misreads a trailing "%").
static BASE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?P<name>[A-Za-z][A-Za-z' ]*?)\s+(?P<num>-?\s*{NUMBER_PATTERN})\s*(?P<pct>%|f)?$"))
        .expect("BASE_LINE pattern is valid")
});
/// "Rarity: X" or "Rarity:X".
static RARITY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^rar[a-z]*\W*(?P<word>[a-z]*)").expect("RARITY_LINE pattern is valid"));
/// A leading "- " or trailing " -" (a bullet or line-wrap dash OCR left attached).
static EDGE_DASHES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-\s+|\s+-$").expect("EDGE_DASHES pattern is valid"));

/// Unicode dashes OCR returns for the game's minus sign, folded to ASCII '-'.
fn translate_dashes(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{2212}' | '\u{2012}' | '\u{2013}' | '\u{2014}' => '-',
            other => other,
        })
        .collect()
}

/// Digits OCR sometimes reads as similar-looking letters.
fn translate_digit_slips(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'I' | 'l' | '|' => '1',
            'O' | 'o' => '0',
            other => other,
        })
        .collect()
}

/// A name or title with only its letters kept, lowercased, for matching against the name tables.
fn norm(text: &str) -> String {
    text.to_lowercase().chars().filter(char::is_ascii_lowercase).collect()
}

/// A stat line ready for the roll/base regexes: dash variants folded to '-', then a stray leading
/// or trailing dash (from a misread bullet or line-wrap) trimmed off.
fn clean_line(text: &str) -> String {
    let translated = translate_dashes(text);
    let trimmed = translated.trim();
    EDGE_DASHES.replace_all(trimmed, "").trim().to_string()
}

/// Python's `round()`: nearest integer, ties rounding to even (not away from zero). `f64::round`
/// already rounds half away from zero like C's `round()`, so this only has to special-case an
/// exact tie, exactly as CPython's own `float.__round__` does.
fn python_round(x: f64) -> f64 {
    let rounded = x.round();
    if (x - rounded).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        rounded
    }
}

/// A captured number to game-data units: spaces dropped, digit slips and comma decimals
/// corrected, ×10 for a percent stat, then rounded half to even. `None` only if the regexes that
/// feed this ever capture something that isn't actually a number.
fn value(num: &str, percent: bool) -> Option<i64> {
    let no_spaces: String = num.chars().filter(|&c| c != ' ').collect();
    let corrected = translate_digit_slips(&no_spaces).replace(',', ".");
    let number: f64 = corrected.parse().ok()?;
    let scale = if percent { PERCENT_SCALE as f64 } else { 1.0 };
    Some(python_round(number * scale) as i64)
}

/// `table`'s value for `key`, or for the entry whose key best fuzzy-matches `key` at `cutoff`.
fn closest<V: Copy>(table: &[(&'static str, V)], key: &str, cutoff: f64) -> Option<V> {
    if let Some(&(_, v)) = table.iter().find(|(k, _)| *k == key) {
        return Some(v);
    }
    let candidates: Vec<&str> = table.iter().map(|(k, _)| *k).collect();
    let best = get_close_matches(key, &candidates, 1, cutoff).into_iter().next()?;
    table.iter().find(|(k, _)| *k == best).map(|(_, v)| v).copied()
}

/// The same lookup as `closest`, over a runtime-built name table (`ItemIndex`'s per-rarity maps).
fn closest_owned<'a>(table: &'a HashMap<String, String>, key: &str, cutoff: f64) -> Option<&'a str> {
    if let Some(v) = table.get(key) {
        return Some(v.as_str());
    }
    let candidates: Vec<&str> = table.keys().map(String::as_str).collect();
    let best = get_close_matches(key, &candidates, 1, cutoff).into_iter().next()?;
    table.get(best).map(String::as_str)
}

/// Which kind of stat line it was: "Name value" (`Base`) or "+value Name" (`Roll`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatKind {
    Base,
    Roll,
}

/// A stat line, once its name resolved to a known stat id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStat {
    pub kind: StatKind,
    pub id: String,
    pub value: i64,
}

/// The flat or percent stat id a roll's name means, or `None` when the name doesn't (even fuzzily)
/// match anything known.
fn roll_stat_id(key: &str, percent: bool) -> Option<&'static str> {
    let pair = FLAT_OR_PERCENT
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, pair)| *pair)
        .or_else(|| {
            let family = closest(&FLAT_OR_PERCENT_FAMILIES, key, FAMILY_CUTOFF)?;
            FLAT_OR_PERCENT.iter().find(|(k, _)| *k == family).map(|(_, pair)| *pair)
        });
    match pair {
        Some((flat, pct)) => Some(if percent { pct } else { flat }),
        None => closest(&ROLL_NAMES, key, STAT_CUTOFF),
    }
}

/// `(kind, stat_id, value)` for a stat line, or `None` when it isn't one we recognise.
pub fn parse_stat(text: &str) -> Option<ParsedStat> {
    let line = clean_line(text);
    if let Some(caps) = ROLL_LINE.captures(&line) {
        let key = norm(&caps["name"]);
        let percent = caps.name("pct").is_some();
        let id = roll_stat_id(&key, percent)?;
        let value = value(&caps["num"], percent)?;
        return Some(ParsedStat { kind: StatKind::Roll, id: id.to_string(), value });
    }
    let caps = BASE_LINE.captures(&line)?;
    let id = closest(&BASE_NAMES, &norm(&caps["name"]), STAT_CUTOFF)?;
    let percent = caps.name("pct").is_some();
    let value = value(&caps["num"], percent)?;
    Some(ParsedStat { kind: StatKind::Base, id: id.to_string(), value })
}

/// Whether the text is shaped like a stat line, regardless of whether its name is known.
fn looks_like_stat(text: &str) -> bool {
    let line = clean_line(text);
    ROLL_LINE.is_match(&line) || BASE_LINE.is_match(&line)
}

/// OCR pieces on the same line ("Rarity:" + "Epic") joined left to right, top to bottom.
pub fn merge_rows(lines: &[OcrLine]) -> Vec<OcrLine> {
    fn centre(line: &OcrLine) -> f64 {
        f64::from(line.y) + f64::from(line.h) / 2.0
    }

    let mut sorted: Vec<&OcrLine> = lines.iter().collect();
    sorted.sort_by(|a, b| centre(a).total_cmp(&centre(b)));

    let mut rows: Vec<Vec<&OcrLine>> = Vec::new();
    for line in sorted {
        if let Some(row) = rows.last_mut() {
            let first = row[0];
            if (centre(first) - centre(line)).abs() <= f64::from(first.h.max(line.h)) / 2.0 {
                row.push(line);
                continue;
            }
        }
        rows.push(vec![line]);
    }

    rows.into_iter()
        .map(|mut pieces| {
            pieces.sort_by_key(|p| p.x);
            let (mut left, mut top) = (pieces[0].x, pieces[0].y);
            let (mut right, mut bottom) = (pieces[0].x + pieces[0].w, pieces[0].y + pieces[0].h);
            for p in &pieces[1..] {
                left = left.min(p.x);
                top = top.min(p.y);
                right = right.max(p.x + p.w);
                bottom = bottom.max(p.y + p.h);
            }
            let text = pieces.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join(" ");
            OcrLine { text, x: left, y: top, w: right - left, h: bottom - top }
        })
        .collect()
}

/// Item ids by rarity and name, from the game's item catalog.
#[derive(Debug, Default)]
pub struct ItemIndex {
    by_rarity: HashMap<String, HashMap<String, String>>,
}

impl ItemIndex {
    /// From the shipped item catalog. Items with no name, or whose rarity `game_data` couldn't
    /// place, aren't indexed (an item can't be found by a rarity it's never shown under).
    pub fn from_catalog(catalog: &ItemCatalog) -> Self {
        let mut by_rarity: HashMap<String, HashMap<String, String>> = HashMap::new();
        for item in catalog.iter() {
            if item.rarity == Rarity::Unknown || item.name.is_empty() {
                continue;
            }
            by_rarity.entry(item.rarity.name().to_string()).or_default().insert(norm(&item.name), item.id.clone());
        }
        Self { by_rarity }
    }

    /// From plain `(id, name, rarity)` triples, e.g. for tests. `rarity` is raw text, read the way
    /// a DnDTools items.json record's "rarity" field is: normalized, then matched by its first
    /// four letters, so "Uncommon", "uncommon" and "UNCO" all resolve the same.
    pub fn from_triples<'a>(items: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Self {
        let mut by_rarity: HashMap<String, HashMap<String, String>> = HashMap::new();
        for (id, name, rarity) in items {
            if name.is_empty() {
                continue;
            }
            let Some(canonical) = rarity_key(rarity) else { continue };
            by_rarity.entry(canonical.to_string()).or_default().insert(norm(name), id.to_string());
        }
        Self { by_rarity }
    }

    /// The item id whose name best fuzzy-matches `title` among items of this `rarity`.
    pub fn resolve(&self, title: &str, rarity: &str) -> Option<String> {
        let table = self.by_rarity.get(rarity)?;
        closest_owned(table, &norm(title), NAME_CUTOFF).map(str::to_string)
    }
}

/// The rarity a normalized "Rarity: X" line's captured word means, if any.
fn rarity_key(word: &str) -> Option<&'static str> {
    let normalized = norm(word);
    let abbrev = &normalized[..normalized.len().min(4)];
    RARITY_KEYS.iter().find(|(k, _)| *k == abbrev).map(|(_, v)| *v)
}

/// The rarity a merged row reads as a "Rarity: X" line, if it is one.
fn rarity_of(row: &OcrLine) -> Option<&'static str> {
    let caps = RARITY_LINE.captures(row.text.trim())?;
    let word = &caps["word"];
    if word.is_empty() {
        None
    } else {
        rarity_key(word)
    }
}

/// Whether OCR'd text could be an item tooltip at all: every tooltip has a "Rarity: X" line.
pub fn looks_like_tooltip(lines: &[OcrLine]) -> bool {
    merge_rows(lines).iter().any(|row| rarity_of(row).is_some())
}

/// A fully read item tooltip: its title, rarity, resolved item id, and stats in game-data units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTooltip {
    pub title: String,
    pub rarity: String,
    pub item_id: String,
    /// Random rolls, `(stat_id, value)`.
    pub rolls: Vec<(String, i64)>,
    /// Base stats, `(stat_id, value)`.
    pub base: Vec<(String, i64)>,
    /// Stat-like lines whose stat isn't known.
    pub unread: Vec<String>,
}

/// `ParsedTooltip`, or `None` unless a known item's title sits in the title bar (above
/// `title_bottom`, crop pixels) and a "Rarity: X" line follows.
pub fn parse_tooltip(lines: &[OcrLine], index: &ItemIndex, title_bottom: f64) -> Option<ParsedTooltip> {
    let rows = merge_rows(lines);
    let rarity_at = rows.iter().position(|row| rarity_of(row).is_some())?;
    let rarity = rarity_of(&rows[rarity_at])?;

    // The row nearest the rule is the title: try candidates from the rule upward.
    let (title_at, item_id) = rows[..rarity_at]
        .iter()
        .enumerate()
        .filter(|(_, row)| f64::from(row.y) < title_bottom)
        .rev()
        .find_map(|(i, row)| index.resolve(&row.text, rarity).map(|id| (i, id)))?;

    let (mut rolls, mut base, mut unread) = (Vec::new(), Vec::new(), Vec::new());
    for row in &rows[title_at + 1..rarity_at] {
        match parse_stat(&row.text) {
            Some(ParsedStat { kind: StatKind::Roll, id, value }) => rolls.push((id, value)),
            Some(ParsedStat { kind: StatKind::Base, id, value }) => base.push((id, value)),
            None if looks_like_stat(&row.text) => unread.push(row.text.clone()),
            None => {}
        }
    }
    let title = rows[title_at].text.clone();
    Some(ParsedTooltip { title, rarity: rarity.to_string(), item_id, rolls, base, unread })
}
