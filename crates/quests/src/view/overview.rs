//! Merchants at a glance, and what the player can do at each right now: hand in quests that are
//! ready, and bring looted items open quests still need.

use crate::packet_types::MerchantFlag;
use crate::view::board::Entry;
use crate::view::model::{CurrentQuest, MerchantSummary, NextStep, QuestState, StepAction, StepItem};
use crate::view::quest::{objective_item, requirement, Ctx};

/// One merchant's card: counts by state, the quest to work on next, and whether anything waits
/// to be handed in. `chain` is the merchant's quests in chain order.
pub(crate) fn merchant_summary(ctx: &Ctx<'_>, merchant: &str, chain: &[&Entry<'_>]) -> MerchantSummary {
    let count = |state: QuestState| u32::try_from(chain.iter().filter(|entry| entry.state == state).count()).unwrap_or(u32::MAX);
    let ready = count(QuestState::Ready);
    let flag_ready = ctx.board.merchant_flag(merchant).map(MerchantFlag::from_raw) == Some(MerchantFlag::Success);
    MerchantSummary {
        name: merchant.to_string(),
        total: u32::try_from(chain.len()).unwrap_or(u32::MAX),
        done: count(QuestState::Done),
        ready,
        active: count(QuestState::Active),
        available: count(QuestState::Available),
        locked: count(QuestState::Locked),
        tracked: chain.iter().any(|entry| entry.tracked),
        turn_in: ready > 0 || flag_ready,
        current: current_quest(ctx, chain),
        bring: chain.iter().flat_map(|entry| bring_items(ctx, entry)).count().try_into().unwrap_or(u32::MAX),
        seen_at: chain.iter().filter_map(|entry| entry.seen_at).reduce(f64::max),
    }
}

/// The first quest of the chain that can be worked on, else the first one waiting.
fn current_quest(ctx: &Ctx<'_>, chain: &[&Entry<'_>]) -> Option<CurrentQuest> {
    let open = chain.iter().find(|entry| matches!(entry.state, QuestState::Ready | QuestState::Active | QuestState::Available));
    let entry = open.or_else(|| chain.iter().find(|entry| entry.state == QuestState::Locked))?;
    let waits_for = if entry.state == QuestState::Locked {
        entry.quest.prerequisite.as_deref().and_then(|id| ctx.board.get(id)).map(|before| before.quest.title.clone())
    } else {
        None
    };
    Some(CurrentQuest { id: entry.quest.id.clone(), title: entry.quest.title.clone(), state: entry.state, waits_for })
}

/// Items an open quest still needs that the player has looted: how many to bring now.
fn bring_items(ctx: &Ctx<'_>, entry: &Entry<'_>) -> Vec<StepItem> {
    if !matches!(entry.state, QuestState::Active | QuestState::Available) {
        return Vec::new();
    }
    entry
        .quest
        .objectives
        .iter()
        .zip(&entry.objectives)
        .filter(|(objective, status)| objective.kind == "Fetch" && !status.done)
        .filter_map(|(objective, status)| {
            let need = requirement(objective)?;
            let needed = objective.count.unwrap_or(0).saturating_sub(status.submitted);
            let usable = ctx.owned.count(&need, ctx.stash_label).usable;
            (needed > 0 && usable > 0).then(|| StepItem {
                item: objective_item(ctx, &need),
                count: usable.min(needed),
                needed,
                min_rarity: need.min_rarity.map(game_data::Rarity::name),
            })
        })
        .collect()
}

/// What the player can do at `merchant` now: quests to hand in first, then items to bring.
pub(crate) fn next_steps(ctx: &Ctx<'_>, merchant: &str, chain: &[&Entry<'_>]) -> Vec<NextStep> {
    let step = |entry: &Entry<'_>, action: StepAction, items: Vec<StepItem>| NextStep {
        merchant: merchant.to_string(),
        quest_id: entry.quest.id.clone(),
        quest_title: entry.quest.title.clone(),
        action,
        items,
    };
    let turn_ins = chain.iter().filter(|entry| entry.state == QuestState::Ready).map(|entry| step(entry, StepAction::TurnIn, Vec::new()));
    let brings = chain.iter().filter_map(|entry| {
        let items = bring_items(ctx, entry);
        (!items.is_empty()).then(|| step(entry, StepAction::Bring, items))
    });
    turn_ins.chain(brings).collect()
}
