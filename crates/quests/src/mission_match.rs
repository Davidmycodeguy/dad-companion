//! Which catalog objective each mission the game reports belongs to.
//!
//! The game names a quest's missions by content id (`"Fetch_CampfireKit_02"`,
//! `"Kill_Huntress_GoblinMage_01"`, `"Escape_HowlingCrypts_01"`) and lists them in its own order,
//! which is not the catalog's: Woodsman_05's missions come as Bavin, Campfire Kit, Cracked Log
//! while the catalog lists Bavin, Cracked Log, Campfire Kit. So missions are matched by name, not
//! by position: the content id's kind must fit the objective's type, and its name must match what
//! the objective names (item, item type, object, place, monster), allowing a plural, a letter or two
//! of spelling difference ("Guardsman" / "Guardman") and leading words one side leaves out
//! ("Ruins Golem" / "Golem"). A mission whose kind has exactly one objective left takes it. The rest
//! stay unmatched rather than guessed (Cockatrice_01's "Fetch_Cockatrice_01_03" names none of its
//! three items).

use crate::catalog::Objective;
use crate::sorting::{normalize_match_id, strip_grade_suffix};

const EXACT: u32 = 100;
const GRADE_FAMILY: u32 = 95;
const PLURAL: u32 = 90;
/// An objective's name without its leading words is never quite an exact match.
const PARTIAL_NAME_CAP: u32 = 85;
const SPELLING: u32 = 80;
const SPELLING_STEP: u32 = 5;
/// A monster's family ("undead" in "Id.character.undead Skeleton") is a weak match.
const FAMILY_CAP: u32 = 70;
const SUFFIX: u32 = 60;
/// Names shorter than these match too easily by spelling distance or as a suffix.
const MIN_SPELLING_LEN: usize = 6;
const MIN_SUFFIX_LEN: usize = 4;
const MAX_SPELLING_DISTANCE: usize = 2;

/// Content id kinds (lowercase) and the catalog objective type each one fits.
const CONTENT_KINDS: &[(&str, &str)] = &[
    ("fetch", "Fetch"),
    ("kill", "Kill"),
    ("explore", "Explore"),
    ("escape", "Survive"),
    ("survive", "Survive"),
    ("damage", "Damage"),
    ("props", "Props"),
    ("prop", "Props"),
    ("interact", "Props"),
    ("useitem", "Use Item"),
    ("use", "Use Item"),
    ("hold", "Hold"),
];

/// For each content id (in the game's order), the index of the objective it belongs to, or None
/// when that can't be told. No two missions get the same objective.
pub fn match_missions(objectives: &[Objective], content_ids: &[&str]) -> Vec<Option<usize>> {
    let missions: Vec<Content> = content_ids.iter().map(|id| Content::parse(id)).collect();
    let names: Vec<Vec<NameKey>> = objectives.iter().map(objective_names).collect();
    let mut assigned: Vec<Option<usize>> = vec![None; missions.len()];
    let mut taken = vec![false; objectives.len()];

    let mut pairs: Vec<(u32, usize, usize)> = Vec::new();
    for (m, mission) in missions.iter().enumerate() {
        for (o, objective) in objectives.iter().enumerate() {
            if mission.fits(&objective.kind) {
                let score = mission.score(&names[o]);
                if score > 0 {
                    pairs.push((score, m, o));
                }
            }
        }
    }
    // Best matches first; ties keep the game's order, then the catalog's.
    pairs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, m, o) in pairs {
        if assigned[m].is_none() && !taken[o] {
            assigned[m] = Some(o);
            taken[o] = true;
        }
    }

    for m in 0..missions.len() {
        if assigned[m].is_some() {
            continue;
        }
        let pick = if missions[m].is_blank() {
            // Nothing to match on: the objective in the same position, as DnDTools did.
            (m < objectives.len() && !taken[m]).then_some(m)
        } else {
            only_open_objective(m, &missions, objectives, &assigned, &taken)
        };
        if let Some(o) = pick {
            assigned[m] = Some(o);
            taken[o] = true;
        }
    }
    assigned
}

/// The one objective left that fits mission `m`, when no other unmatched mission fits it too.
fn only_open_objective(
    m: usize,
    missions: &[Content],
    objectives: &[Objective],
    assigned: &[Option<usize>],
    taken: &[bool],
) -> Option<usize> {
    let mut open = (0..objectives.len()).filter(|&o| !taken[o] && missions[m].fits(&objectives[o].kind));
    let candidate = open.next()?;
    if open.next().is_some() {
        return None;
    }
    let rivals = missions
        .iter()
        .enumerate()
        .filter(|&(other, mission)| other != m && assigned[other].is_none() && mission.fits(&objectives[candidate].kind))
        .count();
    (rivals == 0).then_some(candidate)
}

/// A mission's content id, taken apart.
struct Content {
    /// The objective type its kind fits; None when the id has no known kind.
    kind: Option<&'static str>,
    /// The name part (kind and trailing numbers removed), whole and without each leading word,
    /// normalized: "Kill_Huntress_GoblinMage_01" gives "huntressgoblinmage" and "goblinmage".
    phrases: Vec<String>,
}

impl Content {
    fn parse(content_id: &str) -> Self {
        let tokens: Vec<&str> = content_id.split(['_', ' ', '-']).filter(|t| !t.is_empty()).collect();
        let (kind, rest) = match tokens.split_first() {
            Some((first, rest)) if content_kind(first).is_some() => (content_kind(first), rest),
            _ => (None, &tokens[..]),
        };
        let mut core = rest.to_vec();
        while core.last().is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit())) {
            core.pop();
        }
        let phrases =
            (0..core.len()).map(|start| normalize_match_id(&core[start..].concat())).filter(|p| !p.is_empty()).collect();
        Self { kind, phrases }
    }

    fn is_blank(&self) -> bool {
        self.kind.is_none() && self.phrases.is_empty()
    }

    fn fits(&self, objective_kind: &str) -> bool {
        match self.kind {
            Some(kind) => kind.eq_ignore_ascii_case(objective_kind.trim()),
            None => true,
        }
    }

    fn score(&self, names: &[NameKey]) -> u32 {
        self.phrases
            .iter()
            .flat_map(|phrase| names.iter().map(move |name| similarity(phrase, &name.key).min(name.cap)))
            .max()
            .unwrap_or(0)
    }
}

fn content_kind(token: &str) -> Option<&'static str> {
    let lower = token.to_ascii_lowercase();
    CONTENT_KINDS.iter().find(|(name, _)| *name == lower).map(|&(_, kind)| kind)
}

/// One name an objective goes by, normalized, with the best score a match on it can earn.
struct NameKey {
    key: String,
    cap: u32,
}

/// What an objective names: its item or item type, object, place and monster, each whole and
/// without its leading words, plus a monster's family.
fn objective_names(objective: &Objective) -> Vec<NameKey> {
    let mut names = Vec::new();
    for field in [&objective.item_id, &objective.item_type, &objective.interact, &objective.module].into_iter().flatten() {
        push_name(&mut names, field);
    }
    if let Some(monster) = &objective.monster {
        let (words, family) = split_monster(monster);
        push_name(&mut names, &words);
        if let Some(family) = family {
            names.push(NameKey { key: normalize_match_id(&family), cap: FAMILY_CAP });
        }
    }
    names.retain(|name| !name.key.is_empty());
    names
}

/// The whole name, then the name without each leading word ("Ruins Golem" also as "Golem").
fn push_name(names: &mut Vec<NameKey>, name: &str) {
    let words: Vec<&str> = name.split_whitespace().collect();
    for start in 0..words.len() {
        let cap = if start == 0 { EXACT } else { PARTIAL_NAME_CAP };
        names.push(NameKey { key: normalize_match_id(&words[start..].concat()), cap });
    }
}

/// A catalog monster name split into its words and its family: "Id.character.undead Skeleton" is
/// ("Skeleton", Some("undead")); "Goblin Archer" is ("Goblin Archer", None).
pub(crate) fn split_monster(raw: &str) -> (String, Option<String>) {
    let mut family = None;
    let mut words = Vec::new();
    for token in raw.split_whitespace() {
        if token.contains('.') {
            family = token.rsplit('.').next().filter(|last| !last.is_empty()).map(str::to_string);
        } else {
            words.push(token);
        }
    }
    (words.join(" "), family)
}

/// How well two normalized names match (0: not at all).
fn similarity(phrase: &str, key: &str) -> u32 {
    if phrase == key {
        return EXACT;
    }
    if strip_grade_suffix(phrase).as_deref() == Some(key) || strip_grade_suffix(key).as_deref() == Some(phrase) {
        return GRADE_FAMILY;
    }
    if is_plural_of(phrase, key) || is_plural_of(key, phrase) {
        return PLURAL;
    }
    let shorter = phrase.len().min(key.len());
    if shorter >= MIN_SPELLING_LEN {
        if let Some(distance) = spelling_distance(phrase, key, MAX_SPELLING_DISTANCE) {
            return SPELLING - SPELLING_STEP * distance as u32;
        }
    }
    if shorter >= MIN_SUFFIX_LEN && (phrase.ends_with(key) || key.ends_with(phrase)) {
        return SUFFIX;
    }
    0
}

fn is_plural_of(plural: &str, singular: &str) -> bool {
    plural.strip_prefix(singular).is_some_and(|rest| rest == "s" || rest == "es")
}

/// The edit distance between two ASCII names when it is at most `max`.
fn spelling_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, &left) in a.iter().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, &right) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(left != right);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    let distance = previous[b.len()];
    (distance <= max).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn objective(value: serde_json::Value) -> Objective {
        serde_json::from_value(value).expect("valid objective")
    }
    fn fetch(item: &str) -> Objective {
        objective(json!({"type": "Fetch", "count": 1, "item_id": item}))
    }
    fn kill(monster: &str) -> Objective {
        objective(json!({"type": "Kill", "count": 1, "monster": monster}))
    }

    #[test]
    fn missions_match_by_name_whatever_order_the_game_lists_them_in() {
        // Woodsman_05 and GoblinMerchant_01 as the game really sent them.
        let woodsman = [fetch("Bavin"), fetch("CrackedLog"), fetch("CampfireKit")];
        let missions = ["Fetch_Bavin_02", "Fetch_CampfireKit_02", "Fetch_CrackedLog_01"];
        assert_eq!(match_missions(&woodsman, &missions), vec![Some(0), Some(2), Some(1)]);

        let goblins = [kill("Goblin Axeman"), kill("Goblin Warrior"), kill("Goblin Archer")];
        let missions = ["Kill_GoblinAxeman_01", "Kill_GoblinArcher_01", "Kill_GoblinWarrior_01"];
        assert_eq!(match_missions(&goblins, &missions), vec![Some(0), Some(2), Some(1)]);
    }

    #[test]
    fn plurals_spelling_and_leading_words_still_match() {
        let tailor = [fetch("OldCloth"), fetch("RottenFluids")];
        assert_eq!(match_missions(&tailor, &["Fetch_RottenFluid_01"]), vec![Some(1)]);
        assert_eq!(match_missions(&[fetch("Bandage")], &["Fetch_Bandages_01"]), vec![Some(0)]);

        let skeletons = [kill("Skeleton Footman"), kill("Skeleton Guardman"), kill("Skeleton Archer")];
        let missions = ["Kill_SkeletonGuardsman_01", "Kill_SkeletonArcher_01", "Kill_SkeletonFootman_01"];
        assert_eq!(match_missions(&skeletons, &missions), vec![Some(1), Some(2), Some(0)]);

        let tailor_04 = [kill("Ruins Golem"), kill("Dire Wolf")];
        assert_eq!(match_missions(&tailor_04, &["Kill_DireWolf_01", "Kill_Golem_01"]), vec![Some(1), Some(0)]);
        let huntress = [fetch("BatWing")];
        assert_eq!(match_missions(&huntress, &["Fetch_Huntress_BatWing_01"]), vec![Some(0)]);
    }

    #[test]
    fn monster_families_and_item_types_match() {
        let daily = [kill("Id.character.undead Skeleton")];
        assert_eq!(match_missions(&daily, &["Kill_Huntress_Skeleton_01"]), vec![Some(0)]);
        let tutorial = [kill("Id.character.undead")];
        assert_eq!(match_missions(&tutorial, &["Kill_TavernMaster_Tuto_Undead_01"]), vec![Some(0)]);

        let squire = [
            objective(json!({"type": "Fetch", "count": 1, "item_id": null, "rarity": "Uncommon", "item_type": "Armor"})),
            objective(json!({"type": "Fetch", "count": 1, "item_id": null, "rarity": "Uncommon", "item_type": "Weapon"})),
        ];
        assert_eq!(match_missions(&squire, &["Fetch_Weapon_04", "Fetch_Armor_03"]), vec![Some(1), Some(0)]);
    }

    #[test]
    fn the_only_objective_of_a_kind_takes_an_unnamed_mission() {
        let explore = [objective(json!({"type": "Explore", "count": 1, "module": "Ruins Great Hall 03 Destroyed"}))];
        assert_eq!(match_missions(&explore, &["Explore_Barracks_01"]), vec![Some(0)]);

        let endure = [objective(json!({"type": "Survive", "count": 6})), kill("Skeleton Champion")];
        let missions = ["Kill_SkeletonChampion_01", "Escape_Crypts_01"];
        assert_eq!(match_missions(&endure, &missions), vec![Some(1), Some(0)]);
    }

    #[test]
    fn missions_that_name_nothing_are_left_unmatched() {
        // Cockatrice_01: three items, three missions that name none of them.
        let cockatrice = [fetch("CeremonialDagger"), fetch("GemNecklace"), fetch("ExtraThickPelts")];
        let missions = ["Fetch_Cockatrice_01_03", "Fetch_Cockatrice_01_01", "Fetch_Cockatrice_01_02"];
        assert_eq!(match_missions(&cockatrice, &missions), vec![None, None, None]);

        // A kind with no objective of that type never matches.
        assert_eq!(match_missions(&[fetch("Bandage")], &["Kill_Bandage_01"]), vec![None]);
    }

    #[test]
    fn concrete_grades_blank_ids_and_duplicates() {
        let bandages = [fetch("Bandage"), fetch("Bandage")];
        assert_eq!(match_missions(&bandages, &["Bandage_4001", "Bandage_1001"]), vec![Some(0), Some(1)]);
        assert_eq!(match_missions(&bandages, &["", ""]), vec![Some(0), Some(1)]);
        assert_eq!(match_missions(&[], &["Fetch_Bandage_01"]), vec![None]);
    }

    #[test]
    fn spelling_distance_is_bounded() {
        assert_eq!(spelling_distance("guardsman", "guardman", 2), Some(1));
        assert_eq!(spelling_distance("axeman", "maceman", 2), Some(2));
        assert_eq!(spelling_distance("archer", "warrior", 2), None);
        assert_eq!(split_monster("Id.character.beast Mimic"), ("Mimic".to_string(), Some("beast".to_string())));
    }
}
