//! One quest as the page shows it: objectives with their items and what the player owns of them,
//! rewards, and what the quest waits for.

use game_data::{ItemCatalog, Rarity};

use crate::catalog::{Objective, Reward};
use crate::items::ItemFamilyIndex;
use crate::text::normalize_item_name;
use crate::view::board::{Board, Entry};
use crate::view::labels::{objective_label, rarity_word, reward_label, without_grade};
use crate::view::model::{ItemRef, ObjectiveView, PrerequisiteView, QuestView, RewardView};
use crate::view::owned::{OwnedIndex, Requirement};

/// The item id that pays out gold (shown as gold rather than an item).
const GOLD_COINS: &str = "GoldCoins";

/// What building views needs besides the board.
pub(crate) struct Ctx<'a> {
    pub board: &'a Board<'a>,
    pub owned: &'a OwnedIndex<'a>,
    pub items: &'a ItemCatalog,
    pub families: &'a ItemFamilyIndex,
    pub stash_label: &'a dyn Fn(u32) -> String,
}

/// Objectives that take items: brought to the merchant, or used in the dungeon.
pub(crate) fn takes_items(objective: &Objective) -> bool {
    matches!(objective.kind.as_str(), "Fetch" | "Use Item")
}

/// What an item objective asks for, for counting what the player owns.
pub(crate) fn requirement(objective: &Objective) -> Option<Requirement<'_>> {
    if !takes_items(objective) {
        return None;
    }
    let item_id = objective.item_id.as_deref().filter(|id| !id.is_empty());
    let item_type = objective.item_type.as_deref().filter(|t| !t.is_empty());
    (item_id.is_some() || item_type.is_some()).then(|| Requirement {
        item_id,
        item_type,
        min_rarity: rarity_word(objective.rarity.as_deref()),
    })
}

pub(crate) fn quest_view(ctx: &Ctx<'_>, entry: &Entry<'_>) -> QuestView {
    let quest = entry.quest;
    QuestView {
        id: quest.id.clone(),
        title: if quest.title.is_empty() { quest.id.clone() } else { quest.title.clone() },
        text: quest.text.clone().filter(|text| !text.trim().is_empty()),
        dungeons: quest.dungeons.clone(),
        state: entry.state,
        source: entry.source,
        time_limit: entry.time_limit,
        prerequisite: prerequisite_view(ctx.board, entry),
        objectives: objective_views(ctx, entry),
        rewards: quest.rewards.iter().map(|reward| reward_view(ctx.items, reward)).collect(),
        unmatched: entry.unmatched,
        seen_at: entry.seen_at,
    }
}

fn prerequisite_view(board: &Board<'_>, entry: &Entry<'_>) -> Option<PrerequisiteView> {
    let before = board.get(entry.quest.prerequisite.as_deref()?)?;
    Some(PrerequisiteView {
        id: before.quest.id.clone(),
        title: before.quest.title.clone(),
        merchant: before.merchant.clone(),
        done: before.state == crate::view::model::QuestState::Done,
    })
}

fn objective_views(ctx: &Ctx<'_>, entry: &Entry<'_>) -> Vec<ObjectiveView> {
    let quest = entry.quest;
    let mut survives = 0usize;
    quest
        .objectives
        .iter()
        .zip(&entry.objectives)
        .enumerate()
        .map(|(index, (objective, status))| {
            let place = if objective.kind == "Survive" {
                survives += 1;
                quest.dungeons.get(survives - 1).or_else(|| quest.dungeons.get(index)).map(String::as_str)
            } else {
                None
            };
            let need = requirement(objective);
            let item = need.map(|need| objective_item(ctx, &need));
            let label = objective_label(objective, item.as_ref().map(|item| without_grade(&item.name)), place);
            ObjectiveView {
                index,
                kind: objective.kind.clone(),
                label,
                count: objective.count.unwrap_or(0),
                submitted: status.submitted,
                done: status.done,
                source: status.source,
                item,
                min_rarity: need.and_then(|need| need.min_rarity).map(Rarity::name),
                owned: need.map(|need| ctx.owned.count(&need, ctx.stash_label)),
            }
        })
        .collect()
}

/// The item an objective shows: the grade of the required rarity (or the family's first grade),
/// its slot lit by the required rarity, or not lit when any rarity will do. An objective asking for
/// any item of a type shows the type.
pub(crate) fn objective_item(ctx: &Ctx<'_>, need: &Requirement<'_>) -> ItemRef {
    let Some(item_id) = need.item_id else {
        return ItemRef {
            id: None,
            name: need.item_type.unwrap_or("Item").to_string(),
            rarity: need.min_rarity.map_or("Unknown", Rarity::name),
            icon: None,
        };
    };
    let family = ctx.families.family(item_id);
    let shown_id = match family {
        Some(family) => {
            let at_rarity = need.min_rarity.and_then(|min| {
                family.concrete_item_ids.iter().find(|id| ctx.items.get(id).is_some_and(|item| item.rarity == min)).cloned()
            });
            Some(at_rarity.unwrap_or_else(|| family.representative_item_id.clone()))
        }
        None => ctx.items.get(item_id).map(|_| item_id.to_string()),
    };
    let item = shown_id.as_deref().and_then(|id| ctx.items.get(id));
    let graded = family.is_some_and(|family| family.concrete_item_ids.len() > 1);
    let rarity = match (need.min_rarity, item) {
        (Some(min), _) => min.name(),
        (None, Some(item)) if !graded => item.rarity.name(),
        _ => Rarity::Unknown.name(),
    };
    // Any grade will do: the family's name, not its first grade's ("Diamond", not "Diamond (Cracked)").
    let name = match item {
        Some(item) if graded && need.min_rarity.is_none() => without_grade(&item.name).to_string(),
        Some(item) => item.name.clone(),
        None => normalize_item_name(item_id),
    };
    ItemRef { id: shown_id, name, rarity, icon: item.and_then(|item| item.icon_path.clone()) }
}

fn reward_view(items: &ItemCatalog, reward: &Reward) -> RewardView {
    let item_id = reward.item_id.as_deref().filter(|id| !id.is_empty());
    let item = item_id.map(|id| match items.get(id) {
        Some(item) => ItemRef { id: Some(id.to_string()), name: item.name.clone(), rarity: item.rarity.name(), icon: item.icon_path.clone() },
        None => ItemRef { id: None, name: normalize_item_name(id), rarity: Rarity::Unknown.name(), icon: None },
    });
    RewardView {
        kind: reward.kind.clone(),
        label: reward_label(reward, item.as_ref().map(|item| item.name.as_str())),
        count: reward.count.unwrap_or(0),
        rarity: rarity_word(reward.rarity.as_deref()).map(Rarity::name),
        gold: item_id == Some(GOLD_COINS),
        item,
    }
}
