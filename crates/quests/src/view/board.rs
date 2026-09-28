//! Where every quest stands: the game's latest word on each quest, the player's own ticks, and what
//! follows from the chains. A quest is locked until its prerequisite is done, and a quest the game
//! shows as open (or the player finished) means everything before it in its chain is done.
//!
//! Quests that rotate (the Huntress' dailies and weeklies, seasonal events) all sit in the catalog,
//! but only some are offered at a time: those show only when they were in the newest message that
//! showed their merchant's quests, or while they are in progress.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::catalog::{Objective, Quest, QuestCatalog};
use crate::mission_match::match_missions;
use crate::packet_types::QuestFlag;
use crate::progress::{ObjectiveProgress, QuestProgress};
use crate::sorting::{compute_quest_display_order, QuestOrderInput};
use crate::text::normalize_merchant_name;
use crate::tracked::TrackedState;
use crate::view::labels::{compact_name, time_limit};
use crate::view::model::{GameTally, QuestState, Source};

/// Progress keys packet capture writes: `captured::<quest>::<mission index>::<content id>`.
const CAPTURED_PREFIX: &str = "captured::";

/// One objective's progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectiveStatus {
    pub submitted: u32,
    pub done: bool,
    pub source: Option<Source>,
}

/// One quest and where it stands.
pub(crate) struct Entry<'a> {
    pub quest: &'a Quest,
    /// The merchant's name, variants folded ("Huntress Daily" is "Huntress").
    pub merchant: String,
    pub time_limit: Option<&'static str>,
    /// The game's flag, when it has shown the quest.
    pub flag: Option<QuestFlag>,
    pub objectives: Vec<ObjectiveStatus>,
    pub state: QuestState,
    pub source: Option<Source>,
    pub unmatched: Option<GameTally>,
    pub seen_at: Option<f64>,
    /// The game has reported this quest (now or in DnDTools' saved captures).
    pub tracked: bool,
    pub visible: bool,
    capture: Option<u64>,
}

/// Every quest in the catalog, where it stands.
pub(crate) struct Board<'a> {
    pub entries: Vec<Entry<'a>>,
    index: HashMap<&'a str, usize>,
    /// Merchants the game lists (compact names); empty until it has sent the merchant list.
    active_merchants: HashSet<String>,
    merchant_flags: BTreeMap<String, u32>,
    pub last_update: Option<f64>,
}

/// A mission DnDTools saved in the progress file.
struct SavedMission {
    index: usize,
    content_id: String,
    value: u32,
    completed: bool,
}

/// The game's word on one quest.
struct GameQuest {
    flag: Option<QuestFlag>,
    /// Per objective: what the game counted, when a mission was matched to it.
    values: Vec<Option<u32>>,
    unmatched: Option<GameTally>,
    capture: Option<u64>,
    seen_at: Option<f64>,
    tracked: bool,
}

impl<'a> Board<'a> {
    /// `active_merchants` are the game's merchant ids from its last merchant list.
    pub fn build(catalog: &'a QuestCatalog, progress: &QuestProgress, tracked: &TrackedState, active_merchants: &[String]) -> Self {
        let manual = manual_objectives(progress);
        let saved = saved_missions(progress);
        let entries: Vec<Entry<'a>> = catalog
            .iter()
            .map(|quest| {
                let game = game_quest(quest, tracked, &saved);
                let objectives: Vec<ObjectiveStatus> = quest
                    .objectives
                    .iter()
                    .enumerate()
                    .map(|(i, objective)| {
                        let value = game.values.get(i).copied().flatten();
                        objective_status(objective, value, game.flag, manual.get(&(quest.id.clone(), i)))
                    })
                    .collect();
                let tallied = game.unmatched.is_some_and(|tally| tally.submitted > 0);
                let (state, source) = base_state(game.flag, &objectives, tallied);
                let game_done = matches!(state, QuestState::Done | QuestState::Ready) && source == Some(Source::Game);
                Entry {
                    quest,
                    merchant: normalize_merchant_name(Some(&quest.merchant)),
                    time_limit: time_limit(&quest.merchant, &quest.id),
                    flag: game.flag,
                    objectives,
                    state,
                    source,
                    unmatched: if game_done { None } else { game.unmatched },
                    seen_at: game.seen_at,
                    tracked: game.tracked,
                    visible: true,
                    capture: game.capture,
                }
            })
            .collect();
        let index = entries.iter().enumerate().map(|(i, entry)| (entry.quest.id.as_str(), i)).collect();
        let mut board = Self {
            entries,
            index,
            active_merchants: active_merchants.iter().map(|id| compact_name(id)).filter(|id| !id.is_empty()).collect(),
            merchant_flags: tracked.merchant_flags.clone(),
            last_update: (tracked.last_update > 0.0).then_some(tracked.last_update),
        };
        board.imply_done();
        board.lock();
        board.set_visibility();
        board
    }

    pub fn get(&self, quest_id: &str) -> Option<&Entry<'a>> {
        self.index.get(quest_id).map(|&i| &self.entries[i])
    }

    fn prerequisite_of(&self, i: usize) -> Option<usize> {
        self.entries[i].quest.prerequisite.as_deref().and_then(|id| self.index.get(id).copied())
    }
}

impl<'a> Board<'a> {
    /// Everything before a quest the game shows as open, or that is done, is done.
    fn imply_done(&mut self) {
        let evidence: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.entries[i].state == QuestState::Done || opened_by_game(self.entries[i].flag))
            .collect();
        for start in evidence {
            let mut visited = HashSet::from([start]);
            let mut next = self.prerequisite_of(start);
            while let Some(i) = next {
                if !visited.insert(i) {
                    break;
                }
                let entry = &mut self.entries[i];
                if entry.state != QuestState::Done {
                    entry.state = QuestState::Done;
                    entry.source = Some(Source::Implied);
                    entry.unmatched = None;
                    for (status, objective) in entry.objectives.iter_mut().zip(&entry.quest.objectives) {
                        if !status.done {
                            *status = ObjectiveStatus {
                                submitted: objective.count.unwrap_or(0),
                                done: true,
                                source: Some(Source::Implied),
                            };
                        }
                    }
                }
                next = self.prerequisite_of(i);
            }
        }
    }

    /// A quest waits for its prerequisite, unless the game already shows it open.
    fn lock(&mut self) {
        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            if matches!(entry.state, QuestState::Done | QuestState::Ready | QuestState::Locked) || opened_by_game(entry.flag) {
                continue;
            }
            if self.prerequisite_of(i).is_some_and(|p| self.entries[p].state != QuestState::Done) {
                self.entries[i].state = QuestState::Locked;
                self.entries[i].source = None;
            }
        }
    }

    /// Rotating quests show only while offered: in the newest message that showed their
    /// merchant's quests, or in progress.
    fn set_visibility(&mut self) {
        let mut latest: HashMap<String, u64> = HashMap::new();
        for entry in &self.entries {
            if let Some(capture) = entry.capture.filter(|&c| c > 0) {
                let newest = latest.entry(entry.merchant.clone()).or_default();
                *newest = (*newest).max(capture);
            }
        }
        for entry in &mut self.entries {
            // A placeholder for a quest not in the game yet (DnDTools hid these too).
            if entry.quest.title.trim().is_empty() && entry.quest.text.is_none() {
                entry.visible = false;
                continue;
            }
            if entry.time_limit.is_none() {
                continue;
            }
            let offered = entry.capture.is_some_and(|c| c > 0 && latest.get(&entry.merchant) == Some(&c));
            let under_way = entry.capture.is_some() && matches!(entry.state, QuestState::Active | QuestState::Ready);
            entry.visible = offered || under_way;
        }
    }

    /// Merchants with quests to show, in the order the game introduces them (their first quest's
    /// catalog order). Once the game has listed its merchants, only those (and merchants it has
    /// shown quests for) are shown.
    pub fn merchants(&self) -> Vec<String> {
        let mut first: HashMap<&str, (u32, bool)> = HashMap::new();
        for entry in self.entries.iter().filter(|entry| entry.visible) {
            let order = entry.quest.order.unwrap_or(u32::MAX);
            let slot = first.entry(entry.merchant.as_str()).or_insert((order, false));
            slot.0 = slot.0.min(order);
            slot.1 |= entry.tracked;
        }
        let mut merchants: Vec<(&str, u32)> = first
            .into_iter()
            .filter(|(name, (_, tracked))| {
                self.active_merchants.is_empty() || *tracked || self.active_merchants.contains(&compact_name(name))
            })
            .map(|(name, (order, _))| (name, order))
            .collect();
        merchants.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
        merchants.into_iter().map(|(name, _)| name.to_string()).collect()
    }

    /// A merchant's visible quests, each after its prerequisite (ties in catalog order).
    pub fn chain(&self, merchant: &str) -> Vec<&Entry<'a>> {
        let entries: Vec<&Entry<'a>> =
            self.entries.iter().filter(|entry| entry.visible && entry.merchant.eq_ignore_ascii_case(merchant)).collect();
        let prerequisites: Vec<Vec<String>> = entries.iter().map(|entry| entry.quest.prerequisite.iter().cloned().collect()).collect();
        let ties: Vec<String> = entries.iter().map(|entry| format!("{:010}", entry.quest.order.unwrap_or(u32::MAX))).collect();
        let order = compute_quest_display_order(entries.iter().enumerate().map(|(i, entry)| QuestOrderInput {
            key: &entry.quest.id,
            title: &ties[i],
            prerequisites: &prerequisites[i],
        }));
        let mut sorted = entries;
        sorted.sort_by_key(|entry| order.get(&entry.quest.id).copied().unwrap_or(usize::MAX));
        sorted
    }

    /// The game's flag for a merchant (its merchant list), by catalog name.
    pub fn merchant_flag(&self, merchant: &str) -> Option<u32> {
        let wanted = compact_name(merchant);
        self.merchant_flags.iter().find(|(id, _)| compact_name(id) == wanted).map(|(_, &flag)| flag)
    }
}

/// The game shows the quest open (offered, under way or done): its prerequisites are done.
fn opened_by_game(flag: Option<QuestFlag>) -> bool {
    matches!(flag, Some(QuestFlag::Progress | QuestFlag::Success | QuestFlag::Complete | QuestFlag::Available))
}

/// The player's own ticks and counts (from DnDTools or this app), by quest id and objective index.
fn manual_objectives(progress: &QuestProgress) -> HashMap<(String, usize), ObjectiveProgress> {
    let mut manual: HashMap<(String, usize), ObjectiveProgress> = HashMap::new();
    for (key, entry) in &progress.objectives {
        if key.starts_with(CAPTURED_PREFIX) {
            continue;
        }
        let Some(target) = manual_target(key, entry) else { continue };
        let slot = manual.entry(target).or_default();
        slot.submitted = slot.submitted.max(entry.submitted);
        slot.completed |= entry.completed;
    }
    manual
}

/// The quest and objective a manual entry is about: its own fields, else DnDTools' key layout
/// `<quest>::<type>::<index>::<target>`.
fn manual_target(key: &str, entry: &ObjectiveProgress) -> Option<(String, usize)> {
    let mut parts = key.split("::");
    let key_quest = parts.next().filter(|id| !id.is_empty());
    let key_index = parts.nth(1).and_then(|index| index.parse::<usize>().ok());
    let quest_id = entry.quest_id.clone().or_else(|| key_quest.map(str::to_string))?;
    let index = match entry.objective_index {
        Some(index) => usize::try_from(index).ok()?,
        None => key_index?,
    };
    Some((quest_id, index))
}

/// Missions saved from the game in the progress file, by quest id, in the game's order.
fn saved_missions(progress: &QuestProgress) -> HashMap<String, Vec<SavedMission>> {
    let mut missions: HashMap<String, Vec<SavedMission>> = HashMap::new();
    for (key, entry) in &progress.objectives {
        let Some(rest) = key.strip_prefix(CAPTURED_PREFIX) else { continue };
        let mut parts = rest.splitn(3, "::");
        let (Some(quest_id), Some(index), Some(content_id)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Ok(index) = index.parse::<usize>() else { continue };
        missions.entry(quest_id.to_string()).or_default().push(SavedMission {
            index,
            content_id: content_id.to_string(),
            value: entry.submitted,
            completed: entry.completed,
        });
    }
    for list in missions.values_mut() {
        list.sort_by_key(|mission| mission.index);
    }
    missions
}

/// The game's word on `quest`: what it last showed (this session or before a restart), else what
/// the progress file saved from it (a quest whose saved missions are all complete counts as done).
fn game_quest(quest: &Quest, tracked: &TrackedState, saved: &HashMap<String, Vec<SavedMission>>) -> GameQuest {
    if let Some(entry) = tracked.quests.get(&quest.id) {
        let missions: Vec<(&str, u32)> = entry
            .quest
            .missions
            .iter()
            .map(|m| (m.content_id.as_str(), u32::try_from(m.current_value).unwrap_or(0)))
            .collect();
        let flag = match QuestFlag::from_raw(entry.quest.quest_flag) {
            QuestFlag::None | QuestFlag::Unknown(_) => None,
            flag => Some(flag),
        };
        return matched(quest, flag, &missions, Some(entry.capture), Some(entry.seen_at));
    }
    match saved.get(&quest.id) {
        Some(list) if !list.is_empty() => {
            let missions: Vec<(&str, u32)> = list.iter().map(|m| (m.content_id.as_str(), m.value)).collect();
            let flag = list.iter().all(|m| m.completed).then_some(QuestFlag::Complete);
            matched(quest, flag, &missions, None, None)
        }
        _ => GameQuest {
            flag: None,
            values: vec![None; quest.objectives.len()],
            unmatched: None,
            capture: None,
            seen_at: None,
            tracked: false,
        },
    }
}

/// Ties each mission's count to its objective; what can't be tied is summed into a tally.
fn matched(quest: &Quest, flag: Option<QuestFlag>, missions: &[(&str, u32)], capture: Option<u64>, seen_at: Option<f64>) -> GameQuest {
    let ids: Vec<&str> = missions.iter().map(|&(id, _)| id).collect();
    let mut values = vec![None; quest.objectives.len()];
    let mut loose: Option<u32> = None;
    for (mission, assigned) in match_missions(&quest.objectives, &ids).into_iter().enumerate() {
        match assigned {
            Some(objective) => values[objective] = Some(missions[mission].1),
            None => loose = Some(loose.unwrap_or(0) + missions[mission].1),
        }
    }
    let unmatched = loose.and_then(|submitted| {
        let needed: u32 = quest
            .objectives
            .iter()
            .zip(&values)
            .filter(|(_, value)| value.is_none())
            .map(|(objective, _)| objective.count.unwrap_or(0))
            .sum();
        (needed > 0).then_some(GameTally { submitted: submitted.min(needed), needed })
    });
    GameQuest { flag, values, unmatched, capture, seen_at, tracked: true }
}

/// One objective: the game's count or the player's own, whichever says more; done when the game
/// says the quest is met, the count is reached, or the player ticked it.
fn objective_status(
    objective: &Objective,
    game_value: Option<u32>,
    flag: Option<QuestFlag>,
    manual: Option<&ObjectiveProgress>,
) -> ObjectiveStatus {
    let count = objective.count.unwrap_or(0);
    let game_submitted = game_value.unwrap_or(0);
    let game_done =
        matches!(flag, Some(QuestFlag::Success | QuestFlag::Complete)) || (count > 0 && game_submitted >= count);
    let manual_submitted = manual.map_or(0, |m| m.submitted);
    let manual_done = manual.is_some_and(|m| m.completed);
    let submitted = game_submitted.max(manual_submitted);
    let done = game_done || manual_done || (count > 0 && submitted >= count);
    let source = if game_done {
        Some(Source::Game)
    } else if manual_done || manual_submitted > game_submitted {
        Some(Source::Manual)
    } else if game_value.is_some() {
        Some(Source::Game)
    } else {
        None
    };
    let submitted = match (done, count) {
        (_, 0) => submitted,
        (true, _) => count,
        (false, _) => submitted.min(count),
    };
    ObjectiveStatus { submitted, done, source }
}

/// A quest's state from the game's flag and its objectives, before its chain is considered.
/// `tallied`: the game counted progress it couldn't tie to one objective.
fn base_state(flag: Option<QuestFlag>, objectives: &[ObjectiveStatus], tallied: bool) -> (QuestState, Option<Source>) {
    let all_done = !objectives.is_empty() && objectives.iter().all(|o| o.done);
    let ticked = objectives.iter().any(|o| o.done && o.source == Some(Source::Manual));
    let mut started: Vec<Option<Source>> =
        objectives.iter().filter(|o| o.done || o.submitted > 0).map(|o| o.source).collect();
    if tallied {
        started.push(Some(Source::Game));
    }
    let started_by = if started.contains(&Some(Source::Manual)) { Some(Source::Manual) } else { Some(Source::Game) };
    match flag {
        Some(QuestFlag::Complete) => (QuestState::Done, Some(Source::Game)),
        Some(QuestFlag::Success) => (QuestState::Ready, Some(Source::Game)),
        _ if all_done && ticked => (QuestState::Done, Some(Source::Manual)),
        _ if all_done => (QuestState::Ready, Some(Source::Game)),
        Some(QuestFlag::Progress) => (QuestState::Active, Some(Source::Game)),
        Some(QuestFlag::Locked) => (QuestState::Locked, Some(Source::Game)),
        _ if !started.is_empty() => (QuestState::Active, started_by),
        Some(_) => (QuestState::Available, Some(Source::Game)),
        None => (QuestState::Available, None),
    }
}
