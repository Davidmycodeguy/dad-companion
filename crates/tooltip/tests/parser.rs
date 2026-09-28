//! Port of DnDTools' `tests/test_tooltip_parser.py`, 1:1 (same inputs, same expected results),
//! plus a few extra cases: `get_close_matches`/`ratio` verified directly against Python's
//! `difflib` output, an item resolved through the real shipped catalog, and a couple of tooltip
//! shapes (a rarity line with no space, a stat split into two OCR pieces) not in the original file.

use game_data::ItemCatalog;
use tooltip::parser::{
    get_close_matches, looks_like_tooltip, parse_stat, parse_tooltip, ratio, ItemIndex, ParsedStat, ParsedTooltip,
    StatKind,
};
use tooltip::OcrLine;

/// Half-scale crop: the title bar is the top 110 px of a 4K tooltip read.
const TITLE_BOTTOM: f64 = 55.0;

fn lines(rows: &[(&str, i32, i32, i32, i32)]) -> Vec<OcrLine> {
    rows.iter().map(|&(text, x, y, w, h)| OcrLine::new(text, x, y, w, h)).collect()
}

fn index() -> ItemIndex {
    ItemIndex::from_triples([
        ("OccultistRobe_3001", "Occultist Robe", "Uncommon"),
        ("OccultistRobe_4001", "Occultist Robe", "Rare"),
        ("WoolenCap_2001", "Woolen Cap", "Common"),
        ("ArmingSword_2001", "Arming Sword", "Common"),
        ("GemRing_5001", "Gem Ring (Perfect)", "Epic"),
        ("Quarterstaff_3001", "Quarterstaff", "Uncommon"),
    ])
}

fn base(id: &str, value: i64) -> Option<ParsedStat> {
    Some(ParsedStat { kind: StatKind::Base, id: id.to_string(), value })
}

fn roll(id: &str, value: i64) -> Option<ParsedStat> {
    Some(ParsedStat { kind: StatKind::Roll, id: id.to_string(), value })
}

fn based(id: &str, value: i64) -> (String, i64) {
    (id.to_string(), value)
}

// Real Windows OCR output for tooltips captured in game (half-scale 4K crops, 2026-09-28).
fn occultist_robe() -> Vec<OcrLine> {
    lines(&[
        ("Occultist Robe", 98, 19, 162, 18),
        ("Armor Rating 49", 110, 78, 134, 17),
        ("Magic Resistance 50", 95, 105, 165, 17),
        ("Move Speed \u{2014}4", 116, 132, 123, 17),
        ("Vigor 2", 149, 159, 57, 17),
        ("Will 1", 157, 186, 41, 14),
        ("- +2% Undead Damage Bonus", 45, 214, 246, 16),
        ("Required Class.", 116, 261, 126, 17),
        ("Warlock", 146, 287, 65, 12),
        ("Slot Type: Chest", 111, 312, 137, 15),
        ("Armor Type:", 102, 337, 100, 15),
        ("Cloth", 213, 337, 43, 12),
        ("Loot State:", 96, 362, 88, 12),
        ("Handled", 196, 362, 67, 12),
        ("Rarity:", 101, 387, 55, 15),
        ("Uncommon", 167, 387, 90, 12),
        ("A bewitching garment that weaves", 39, 436, 280, 16),
        ("together style and sorcery.", 69, 463, 219, 16),
    ])
}

fn woolen_cap() -> Vec<OcrLine> {
    lines(&[
        ("Woolen Cap", 146, 19, 131, 22),
        ("Armor Rating 21", 145, 78, 131, 17),
        ("Headshot Damage Reduction 7 f", 78, 105, 254, 17),
        ("Move Speed \u{2014}2", 149, 132, 122, 17),
        ("Vigor I", 183, 159, 54, 17),
        ("Dexterity I", 166, 186, 88, 17),
        ("Agility I", 178, 213, 65, 17),
        ("Slot Type: Head(Hat)", 122, 267, 179, 16),
        ("Armor Type: Cloth", 135, 293, 154, 15),
        ("Supplied", 228, 317, 68, 17),
        ("Loot State:", 128, 318, 88, 12),
        ("Rarity:", 144, 343, 55, 15),
        ("Common", 210, 343, 70, 13),
        ("A practical cap easily found by an)", 53, 392, 279, 17),
        ("13 :", 90, 748, 75, 11),
        ("29.3", 303, 748, 29, 12),
    ])
}

fn gem_ring() -> Vec<OcrLine> {
    lines(&[
        ("Gem Ring (Perfect)", 89, 19, 300, 22),
        ("Slot Type: Invalid", 126, 80, 180, 16),
        ("Looted", 222, 105, 60, 14),
        ("Loot State:", 122, 106, 90, 13),
        ("Rarity: Epic", 150, 130, 100, 16),
        ("This gold ring, adorned with a gleaming", 39, 245, 320, 16),
        ("Deal", 11, 416, 40, 14),
    ])
}

/// A fixed UI line under a merchant's name, read as if it were a tooltip.
fn merchant_header() -> Vec<OcrLine> {
    lines(&[
        ("The Collector", 60, 20, 180, 22),
        ("I'm surprised you survived", 40, 90, 240, 16),
        ("Gem Ring", 150, 700, 120, 20),
        ("Rarity", 160, 800, 60, 16),
    ])
}

#[test]
fn rows_merge_pieces_on_the_same_line_left_to_right() {
    let rows = tooltip::parser::merge_rows(&lines(&[
        ("Epic", 167, 387, 50, 12),
        ("Rarity:", 101, 386, 55, 15),
        ("Next", 90, 420, 40, 12),
    ]));
    let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, vec!["Rarity: Epic", "Next"]);
}

#[test]
fn parses_an_uncommon_robe_with_its_random_roll() {
    let tip = parse_tooltip(&occultist_robe(), &index(), TITLE_BOTTOM);
    assert_eq!(
        tip,
        Some(ParsedTooltip {
            title: "Occultist Robe".to_string(),
            rarity: "Uncommon".to_string(),
            item_id: "OccultistRobe_3001".to_string(),
            rolls: vec![based("UndeadDamageMod", 20)],
            base: vec![
                based("ArmorRating", 49),
                based("MagicRegistance", 50),
                based("MoveSpeed", -4),
                based("Vigor", 2),
                based("Will", 1),
            ],
            unread: vec![],
        })
    );
}

#[test]
fn ocr_slips_in_numbers_are_corrected() {
    let tip = parse_tooltip(&woolen_cap(), &index(), TITLE_BOTTOM).expect("parses");
    assert_eq!(tip.item_id, "WoolenCap_2001");
    assert_eq!(
        tip.base,
        vec![
            based("ArmorRating", 21),
            based("HeadshotReductionMod", 70),
            based("MoveSpeed", -2),
            based("Vigor", 1),
            based("Dexterity", 1),
            based("Agility", 1),
        ]
    );
    assert_eq!(tip.rolls, Vec::new());
}

#[test]
fn items_without_stats_still_resolve() {
    let tip = parse_tooltip(&gem_ring(), &index(), TITLE_BOTTOM).expect("parses");
    assert_eq!(tip.item_id, "GemRing_5001");
    assert_eq!(tip.rarity, "Epic");
    assert!(tip.base.is_empty());
    assert!(tip.rolls.is_empty());
}

#[test]
fn the_title_must_sit_in_the_title_bar() {
    assert_eq!(parse_tooltip(&merchant_header(), &index(), TITLE_BOTTOM), None);
}

#[test]
fn nothing_without_a_rarity_line() {
    assert_eq!(parse_tooltip(&occultist_robe()[..6], &index(), TITLE_BOTTOM), None);
}

#[test]
fn rarity_picks_the_right_item_among_same_names() {
    let lines: Vec<OcrLine> = occultist_robe()
        .into_iter()
        .map(|line| if line.text == "Uncommon" { OcrLine::new("Rare", 167, 387, 40, 12) } else { line })
        .collect();
    assert_eq!(parse_tooltip(&lines, &index(), TITLE_BOTTOM).expect("parses").item_id, "OccultistRobe_4001");
}

#[test]
fn a_slightly_misread_title_still_resolves() {
    let mut lines = vec![OcrLine::new("Occu1tist Rohe", 98, 19, 162, 18)];
    lines.extend(occultist_robe().into_iter().skip(1));
    assert_eq!(parse_tooltip(&lines, &index(), TITLE_BOTTOM).expect("parses").item_id, "OccultistRobe_3001");
}

#[test]
fn unknown_items_are_not_guessed() {
    let mut lines = vec![OcrLine::new("Totally New Helm", 98, 19, 162, 18)];
    lines.extend(occultist_robe().into_iter().skip(1));
    assert_eq!(parse_tooltip(&lines, &index(), TITLE_BOTTOM), None);
}

#[test]
fn stat_lines() {
    assert_eq!(parse_stat("Weapon Damage 28"), base("PhysicalWeaponDamage", 28));
    assert_eq!(parse_stat("Armor Penetration 30%"), base("ArmorPenetration", 300));
    assert_eq!(parse_stat("Projectile Damage Reduction 3.5%"), base("ProjectileReductionMod", 35));
    assert_eq!(parse_stat("Move Speed \u{2212}50"), base("MoveSpeed", -50));
    assert_eq!(parse_stat("+5.1% Undead Damage Bonu"), roll("UndeadDamageMod", 51));
    assert_eq!(parse_stat("- +3 Strength -"), roll("Strength", 3));
    assert_eq!(parse_stat("+12 Armor Rating"), roll("ArmorRatingAdd", 12));
    assert_eq!(parse_stat("+2.4% Max Health"), roll("MaxHealthBonus", 24));
    assert_eq!(parse_stat("+6 Max Health"), roll("MaxHealthAdd", 6));
    assert_eq!(parse_stat("Slot Type: Chest"), None);
    assert_eq!(parse_stat("Fighter. Ranger. Bard"), None);
}

#[test]
fn unmapped_stat_lines_are_reported_not_guessed() {
    let mut ls = occultist_robe()[..6].to_vec();
    ls.push(OcrLine::new("+4% Brand New Stat", 45, 214, 246, 16));
    ls.extend(occultist_robe().into_iter().skip(7));
    let tip = parse_tooltip(&ls, &index(), TITLE_BOTTOM).expect("parses");
    assert!(tip.rolls.is_empty());
    assert_eq!(tip.unread, vec!["+4% Brand New Stat".to_string()]);
}

#[test]
fn a_rarity_line_read_without_its_space_still_counts() {
    assert!(looks_like_tooltip(&[OcrLine::new("Rarity:Uncommon", 101, 387, 150, 15)]));
    assert!(looks_like_tooltip(&[OcrLine::new("Rarity: Epic", 101, 387, 150, 15)]));
    assert!(!looks_like_tooltip(&[OcrLine::new("My Listings", 10, 10, 100, 15)]));
}

#[test]
fn memory_capacity_and_other_bonus_rolls_are_read() {
    assert_eq!(parse_stat("+7.1% Memory Capacity Bonus"), roll("MemoryCapacityBonus", 71));
    assert_eq!(parse_stat("+3 Memory Capacity"), roll("MemoryCapacityAdd", 3));
    assert_eq!(parse_stat("+2.5% Max Health Bonus"), roll("MaxHealthBonus", 25));
}

#[test]
fn base_stats_beyond_the_basics_are_read() {
    assert_eq!(parse_stat("Regular Interaction Speed 15.7%"), base("RegularInteractionSpeed", 157));
    assert_eq!(parse_stat("Spell Casting Speed 5%"), base("SpellCastingSpeed", 50));
    assert_eq!(parse_stat("Magical Power 3"), base("MagicalPower", 3));
    assert_eq!(parse_stat("Undead Damage Bonus 4%"), base("UndeadDamageMod", 40));
}

// --- Extra cases beyond the ported Python suite ---

#[test]
fn a_rarity_line_with_no_space_at_all_still_resolves_the_full_tooltip() {
    let lines: Vec<OcrLine> = gem_ring()
        .into_iter()
        .map(|line| if line.text == "Rarity: Epic" { OcrLine::new("Rarity:Epic", 150, 130, 90, 16) } else { line })
        .collect();
    let tip = parse_tooltip(&lines, &index(), TITLE_BOTTOM).expect("parses");
    assert_eq!(tip.item_id, "GemRing_5001");
    assert_eq!(tip.rarity, "Epic");
}

#[test]
fn a_stat_split_into_two_ocr_pieces_on_one_line_still_merges() {
    let rows = lines(&[
        ("Occultist Robe", 98, 19, 162, 18),
        ("Armor Rating", 110, 78, 90, 17),
        ("49", 205, 79, 30, 16),
        ("Rarity: Uncommon", 101, 387, 150, 15),
    ]);
    let tip = parse_tooltip(&rows, &index(), TITLE_BOTTOM).expect("parses");
    assert_eq!(tip.base, vec![based("ArmorRating", 49)]);
}

#[test]
fn resolves_against_the_real_shipped_catalog() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/items.json");
    let catalog = ItemCatalog::load(&path).expect("the shipped catalog loads");
    let idx = ItemIndex::from_catalog(&catalog);
    assert_eq!(idx.resolve("Arming Sword", "Epic"), Some("ArmingSword_5001".to_string()));
    // A slightly misread rarity-appropriate name still resolves through the real catalog too.
    assert_eq!(idx.resolve("Arming Sord", "Epic"), Some("ArmingSword_5001".to_string()));
}

// --- difflib fidelity: exact `ratio`/`get_close_matches` values from CPython's `difflib` ---

#[test]
fn ratio_matches_python_difflib_on_ocr_slips_and_truncated_names() {
    assert_eq!(ratio("weapondamage", "weapondamag"), 0.9565217391304348);
    assert_eq!(ratio("armorpenetration", "armorpenetratio"), 0.967741935483871);
    assert_eq!(ratio("undeaddamagebonus", "undeaddamagebonu"), 0.9696969696969697);
    assert_eq!(ratio("regularinteractionspeed", "regularinteractionspee"), 0.9777777777777777);
    assert_eq!(ratio("occultistrobe", "occutistrohe"), 0.88);
    assert_eq!(ratio("gemringperfect", "gemringperfect"), 1.0);
}

#[test]
fn get_close_matches_matches_python_difflib_on_stat_name_lookups() {
    let base_names = ["weapondamage", "magicweapondamage", "armorrating", "movespeed"];
    assert_eq!(get_close_matches("weapondamag", &base_names, 1, 0.8), vec!["weapondamage"]);
    assert_eq!(get_close_matches("movespee", &base_names, 1, 0.8), vec!["movespeed"]);

    let roll_names = ["undeaddamagebonus", "memorycapacitybonus", "physicalpower", "luck"];
    assert_eq!(get_close_matches("undeaddamagebonu", &roll_names, 1, 0.8), vec!["undeaddamagebonus"]);
    assert_eq!(get_close_matches("brandnewstat", &roll_names, 1, 0.8), Vec::<&str>::new());
}
