//! Quest-list sorting and captured-mission matching, ported from the browser's `static/js/quest.js`
//! (specifically `rebuildQuestDependencyIndex` and the `firstUnclaimedIndex` matcher), which
//! `tests/test_quest_sorting.py` exercises by `eval`-ing slices of that file. The rest of that test
//! file drives page-lifecycle/DOM-cleanup behavior (aborting fetches, removing listeners, ...) that
//! has no equivalent in a backend crate, so only these two portable algorithms were ported.
//!
//! `quest.js` also resolves a quest's free-text `prerequisite` field to another quest's id through a
//! fuzzy alias index (comma-splitting, article/filler-word stripping, substring fallback — see
//! `generateAliasVariants`/`resolvePrerequisiteReferences`). That machinery exists for legacy,
//! loosely-structured quest sources; in the DarkerDB v2 snapshot this crate loads (see
//! [`crate::catalog`]), `prerequisite` is already produced by the same canonicalizer that makes a
//! quest's own `id`, so it already *is* another quest's id. [`compute_quest_display_order`] takes
//! prerequisite keys directly instead of reimplementing that fuzzy resolution.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// One quest's identity for [`compute_quest_display_order`].
pub struct QuestOrderInput<'a> {
    pub key: &'a str,
    /// Used only to break ties deterministically; quests with no title still sort (by key).
    pub title: &'a str,
    /// Keys of quests that must be placed before this one. A key with no matching quest in the
    /// input is ignored, the same way `rebuildQuestDependencyIndex` skips a dependency it can't
    /// resolve.
    pub prerequisites: &'a [String],
}

/// A display-order index (0-based, in the order quests should be shown) for every quest key, with
/// prerequisites always placed before whatever depends on them. Ties — quests that become orderable
/// at the same time — are broken by title (then key) so the result never depends on input order,
/// even under a dependency cycle (whose members are appended last, also title-ordered). Mirrors
/// `rebuildQuestDependencyIndex`'s topological sort.
pub fn compute_quest_display_order<'a, I>(quests: I) -> HashMap<String, usize>
where
    I: IntoIterator<Item = QuestOrderInput<'a>>,
{
    let nodes: Vec<QuestOrderInput<'a>> = quests.into_iter().collect();
    let title_by_key: HashMap<&str, &str> = nodes.iter().map(|node| (node.key, node.title)).collect();
    let known_keys: HashSet<&str> = nodes.iter().map(|node| node.key).collect();
    let compare = |a: &&str, b: &&str| -> Ordering {
        let title_a = title_by_key.get(a).copied().unwrap_or(a);
        let title_b = title_by_key.get(b).copied().unwrap_or(b);
        natural_case_insensitive_compare(title_a, title_b).then_with(|| natural_case_insensitive_compare(a, b))
    };

    let mut indegree: HashMap<&str, usize> = nodes.iter().map(|node| (node.key, 0)).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = nodes.iter().map(|node| (node.key, Vec::new())).collect();
    for node in &nodes {
        for prerequisite in node.prerequisites {
            let prerequisite = prerequisite.as_str();
            if !known_keys.contains(prerequisite) {
                continue;
            }
            dependents.get_mut(prerequisite).expect("known key was seeded above").push(node.key);
            *indegree.get_mut(node.key).expect("known key was seeded above") += 1;
        }
    }

    let mut queue: Vec<&str> = indegree.iter().filter(|(_, &degree)| degree == 0).map(|(&key, _)| key).collect();
    queue.sort_by(compare);

    let mut order: Vec<&str> = Vec::with_capacity(nodes.len());
    let mut processed: HashSet<&str> = HashSet::with_capacity(nodes.len());
    while !queue.is_empty() {
        let node = queue.remove(0);
        order.push(node);
        processed.insert(node);

        // More than one quest can become eligible at once; re-sorting the whole frontier after
        // every step keeps the result independent of API/insertion order (see the module doc).
        let mut newly_eligible = Vec::new();
        for &neighbor in dependents.get(node).into_iter().flatten() {
            if let Some(degree) = indegree.get_mut(neighbor) {
                *degree -= 1;
                if *degree == 0 {
                    newly_eligible.push(neighbor);
                }
            }
        }
        queue.extend(newly_eligible);
        queue.sort_by(compare);
    }

    // A cycle (or a node whose prerequisite never resolves) leaves some nodes unprocessed; append
    // them in the same deterministic order so the UI never crashes or shows nothing for them.
    let mut remaining: Vec<&str> = nodes.iter().map(|node| node.key).filter(|key| !processed.contains(key)).collect();
    remaining.sort_by(compare);

    order.into_iter().chain(remaining).enumerate().map(|(index, key)| (key.to_string(), index)).collect()
}

/// Case-insensitive comparison that compares embedded digit runs numerically (`"item2" <
/// "item10"`), approximating `String.localeCompare(..., {sensitivity: 'base', numeric: true})`.
fn natural_case_insensitive_compare(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a.chars().peekable(), b.chars().peekable());
    loop {
        return match (left.peek().copied(), right.peek().copied()) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(lc), Some(rc)) if lc.is_ascii_digit() && rc.is_ascii_digit() => {
                match take_number(&mut left).cmp(&take_number(&mut right)) {
                    Ordering::Equal => continue,
                    ordering => ordering,
                }
            }
            (Some(lc), Some(rc)) => match lc.to_ascii_lowercase().cmp(&rc.to_ascii_lowercase()) {
                Ordering::Equal => {
                    left.next();
                    right.next();
                    continue;
                }
                ordering => ordering,
            },
        };
    }
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u64 {
    let mut value: u64 = 0;
    while let Some(&digit) = chars.peek().filter(|c| c.is_ascii_digit()) {
        value = value.saturating_mul(10).saturating_add(u64::from(digit.to_digit(10).unwrap_or(0)));
        chars.next();
    }
    value
}

/// The fields of one quest objective [`ObjectiveMatcher`] can match a captured mission against —
/// mirrors the fields quest.js's matcher reads off a DarkerDB objective.
#[derive(Debug, Clone, Default)]
pub struct ObjectiveDescriptor {
    pub item_id: Option<String>,
    pub monster: Option<String>,
    pub monster_type: Option<String>,
    pub interact: Option<String>,
    pub module: Option<String>,
}

/// Reconciles packet-captured mission ids (concrete, e.g. `"SkeletonChampion"`,
/// `"Bandage_4001"`) against a quest's catalog objectives (which name an item *family* like
/// `"Bandage"`, or a spaced display name like `"Skeleton Champion"`), tracking which objectives a
/// mission has already claimed so two missions never map to the same one. Ported from quest.js's
/// `normId`/`matchKeys`/`firstUnclaimedIndex`/`claimedObjectives`.
pub struct ObjectiveMatcher {
    /// Every normalized field value (and its item-family variant) an objective carries, to that
    /// objective's index — built once so each lookup is O(1) instead of rescanning objectives.
    field_index: HashMap<String, Vec<usize>>,
    claimed: HashSet<usize>,
}

impl ObjectiveMatcher {
    pub fn new(objectives: &[ObjectiveDescriptor]) -> Self {
        let mut field_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, objective) in objectives.iter().enumerate() {
            let fields =
                [&objective.item_id, &objective.monster, &objective.monster_type, &objective.interact, &objective.module];
            for field in fields.into_iter().flatten() {
                for key in match_keys(field) {
                    let indices = field_index.entry(key).or_default();
                    if !indices.contains(&index) {
                        indices.push(index);
                    }
                }
            }
        }
        Self { field_index, claimed: HashSet::new() }
    }

    /// The first not-yet-claimed objective index whose fields match `value`: the exact normalized id
    /// is tried first, then its item-family key (so a concrete grade like `"Bandage_4001"` matches an
    /// objective naming the family `"Bandage"`). `None` when nothing unclaimed matches. Mirrors
    /// `firstUnclaimedIndex`.
    pub fn first_unclaimed_index(&self, value: &str) -> Option<usize> {
        for key in match_keys(value) {
            if let Some(indices) = self.field_index.get(&key) {
                if let Some(&candidate) = indices.iter().find(|index| !self.claimed.contains(index)) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Mark an objective index as claimed by a mission, so later lookups skip it.
    pub fn claim(&mut self, index: usize) {
        self.claimed.insert(index);
    }
}

/// Normalize an id for fuzzy comparison: lowercase, then drop everything but ASCII letters/digits —
/// so `"Skeleton Champion"`, `"SkeletonChampion"` and `"skeleton_champion"` all become
/// `"skeletonchampion"`. Mirrors quest.js's `normId`.
pub(crate) fn normalize_match_id(value: &str) -> String {
    value.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect()
}

/// The keys `value` can be looked up under: its normalized id, plus (when present) that id with a
/// trailing item-grade suffix stripped, so a concrete grade matches its family. Mirrors quest.js's
/// `matchKeys`.
fn match_keys(value: &str) -> Vec<String> {
    let normalized = normalize_match_id(value);
    if normalized.is_empty() {
        return Vec::new();
    }
    match strip_grade_suffix(&normalized) {
        Some(family) if family != normalized => vec![normalized, family],
        _ => vec![normalized],
    }
}

/// Strip a trailing item-grade suffix (a digit 1-8 followed by "001") from an already-normalized
/// (lowercase, separator-free) id, e.g. `"bandage4001"` -> `"bandage"`. Mirrors quest.js's
/// `normalized.replace(/[1-8]001$/, '')` — note no underscore, unlike
/// [`crate::items`]'s equivalent, since `matchKeys` normalizes separators away first.
pub(crate) fn strip_grade_suffix(normalized: &str) -> Option<String> {
    let len = normalized.len();
    if len < 4 {
        return None;
    }
    let tail = &normalized[len - 4..];
    let mut chars = tail.chars();
    let grade = chars.next();
    let rest = chars.as_str();
    if rest == "001" && matches!(grade, Some(c) if ('1'..='8').contains(&c)) {
        Some(normalized[..len - 4].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ported from `test_prerequisite_sort_is_stable_when_api_order_changes`: prerequisites always
    /// precede their dependents, siblings sort by title, and a dependency cycle (quest-f/quest-g)
    /// doesn't change the result or crash — regardless of the input array's order.
    #[test]
    fn prerequisite_sort_is_stable_when_api_order_changes() {
        struct Quest {
            id: &'static str,
            title: &'static str,
            depends_on: Vec<String>,
        }
        let quests = [
            Quest { id: "quest-a", title: "Zulu Root", depends_on: vec![] },
            Quest { id: "quest-b", title: "Alpha Root", depends_on: vec![] },
            Quest { id: "quest-c", title: "Beta Child", depends_on: vec!["quest-a".to_string()] },
            Quest { id: "quest-d", title: "Able Child", depends_on: vec!["quest-b".to_string()] },
            Quest { id: "quest-f", title: "Charlie Cycle", depends_on: vec!["quest-g".to_string()] },
            Quest { id: "quest-g", title: "Delta Cycle", depends_on: vec!["quest-f".to_string()] },
        ];
        let expected = ["quest-b", "quest-d", "quest-a", "quest-c", "quest-f", "quest-g"];

        let ordered_keys = |indices: &[usize]| -> Vec<String> {
            let inputs = indices.iter().map(|&i| {
                let quest = &quests[i];
                QuestOrderInput { key: quest.id, title: quest.title, prerequisites: &quest.depends_on }
            });
            let order = compute_quest_display_order(inputs);
            let mut by_index: Vec<(String, usize)> = order.into_iter().collect();
            by_index.sort_by_key(|(_, index)| *index);
            by_index.into_iter().map(|(key, _)| key).collect()
        };

        let original = ordered_keys(&[0, 1, 2, 3, 4, 5]);
        let shuffled = ordered_keys(&[5, 2, 0, 4, 3, 1]);

        assert_eq!(original, expected, "unexpected prerequisite order");
        assert_eq!(shuffled, expected, "API insertion order changed the output");
        let index_of = |key: &str| original.iter().position(|k| k == key).unwrap();
        assert!(index_of("quest-b") < index_of("quest-d"), "a dependent quest preceded its prerequisite");
        assert!(index_of("quest-a") < index_of("quest-c"), "a dependent quest preceded its prerequisite");
    }

    /// Ported from `test_packet_concrete_grade_matches_v2_quest_item_family`: a concrete packet id
    /// resolves to its family objective, a second concrete grade still finds the (still-unclaimed)
    /// duplicate family objective, and plain fuzzy name matching keeps working alongside it.
    #[test]
    fn packet_concrete_grade_matches_v2_quest_item_family() {
        let objectives = vec![
            ObjectiveDescriptor { item_id: Some("Bandage".to_string()), ..Default::default() },
            ObjectiveDescriptor { item_id: Some("Bandage".to_string()), ..Default::default() },
            ObjectiveDescriptor { monster: Some("Skeleton Champion".to_string()), ..Default::default() },
        ];
        let mut matcher = ObjectiveMatcher::new(&objectives);

        let first = matcher.first_unclaimed_index("Bandage_4001");
        assert_eq!(first, Some(0), "concrete grade did not match family objective");
        matcher.claim(first.expect("checked above"));

        let second = matcher.first_unclaimed_index("Bandage_1001");
        assert_eq!(second, Some(1), "duplicate family objective was not available");

        let monster = matcher.first_unclaimed_index("SkeletonChampion");
        assert_eq!(monster, Some(2), "existing fuzzy match regressed");
    }

    #[test]
    fn natural_case_insensitive_compare_orders_embedded_numbers_numerically() {
        assert_eq!(natural_case_insensitive_compare("item2", "item10"), Ordering::Less);
        assert_eq!(natural_case_insensitive_compare("Alpha", "alpha"), Ordering::Equal);
        assert_eq!(natural_case_insensitive_compare("Alpha", "Zulu"), Ordering::Less);
    }
}
