//! The Quests page's views: merchants at a glance with next steps, one merchant's quest chain,
//! and the item checklist. Pure functions of the catalog, the progress file, what the game showed
//! ([`TrackedState`]) and the player's characters, so they are tested without any files.

mod board;
mod items;
mod labels;
pub mod model;
mod overview;
mod owned;
mod quest;

use game_data::ItemCatalog;
use state::Character;

use crate::catalog::QuestCatalog;
use crate::items::ItemFamilyIndex;
use crate::progress::QuestProgress;
use crate::tracked::TrackedState;

pub use labels::compact_name;
pub use model::{
    CurrentQuest, GameTally, ItemNeed, ItemRef, MerchantQuests, MerchantSummary, NeedQuest, NextStep, ObjectiveView,
    Owned, Place, PrerequisiteView, QuestItems, QuestState, QuestView, QuestsOverview, RewardView, Source, StepAction,
    StepItem,
};

use board::Board;
use owned::OwnedIndex;
use quest::Ctx;

/// Everything a view is built from.
pub struct ViewInputs<'a> {
    pub catalog: &'a QuestCatalog,
    pub progress: &'a QuestProgress,
    pub tracked: &'a TrackedState,
    /// The game's merchant ids from its last merchant list; empty until it has sent one.
    pub active_merchants: &'a [String],
    pub characters: &'a [Character],
    pub items: &'a ItemCatalog,
    pub families: &'a ItemFamilyIndex,
    /// Names a stash tab ("Bag", "Stash 2").
    pub stash_label: &'a dyn Fn(u32) -> String,
}

fn with_ctx<T>(inputs: &ViewInputs<'_>, build: impl FnOnce(&Ctx<'_>) -> T) -> T {
    let board = Board::build(inputs.catalog, inputs.progress, inputs.tracked, inputs.active_merchants);
    let owned = OwnedIndex::build(inputs.characters, inputs.items, inputs.families);
    let ctx = Ctx { board: &board, owned: &owned, items: inputs.items, families: inputs.families, stash_label: inputs.stash_label };
    build(&ctx)
}

/// Every merchant with quests to show, and what the player can do now (hand-ins first).
pub fn overview(inputs: &ViewInputs<'_>) -> QuestsOverview {
    with_ctx(inputs, |ctx| {
        let mut merchants = Vec::new();
        let mut next_steps = Vec::new();
        for merchant in ctx.board.merchants() {
            let chain = ctx.board.chain(&merchant);
            merchants.push(overview::merchant_summary(ctx, &merchant, &chain));
            next_steps.extend(overview::next_steps(ctx, &merchant, &chain));
        }
        next_steps.sort_by_key(|step| step.action != StepAction::TurnIn);
        QuestsOverview { merchants, next_steps, last_update: ctx.board.last_update, characters: inputs.characters.len() }
    })
}

/// One merchant's quests in chain order; None when the merchant has no quests to show.
pub fn merchant_quests(inputs: &ViewInputs<'_>, merchant: &str) -> Option<MerchantQuests> {
    with_ctx(inputs, |ctx| {
        let chain = ctx.board.chain(merchant);
        let name = chain.first()?.merchant.clone();
        Some(MerchantQuests {
            merchant: overview::merchant_summary(ctx, &name, &chain),
            quests: chain.iter().map(|entry| quest::quest_view(ctx, entry)).collect(),
        })
    })
}

/// Every item the shown merchants' open quests still need.
pub fn quest_items(inputs: &ViewInputs<'_>) -> QuestItems {
    with_ctx(inputs, |ctx| QuestItems {
        items: items::item_needs(ctx, &ctx.board.merchants()),
        characters: inputs.characters.len(),
    })
}
