//! The item checklist: every item open quests still need, summed over the quests that need it,
//! with what the player owns of it.

use std::collections::HashMap;

use crate::view::model::{ItemNeed, NeedQuest, QuestState};
use crate::view::quest::{objective_item, requirement, Ctx};

/// Items still needed by the quests of `merchants` (in their chain order), by item name. Quests
/// handed in or ready to hand in need nothing more; locked quests are included (marked by state)
/// so the page can leave them out.
pub(crate) fn item_needs(ctx: &Ctx<'_>, merchants: &[String]) -> Vec<ItemNeed> {
    let mut needs: Vec<ItemNeed> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for merchant in merchants {
        for entry in ctx.board.chain(merchant) {
            if matches!(entry.state, QuestState::Done | QuestState::Ready) {
                continue;
            }
            for (objective, status) in entry.quest.objectives.iter().zip(&entry.objectives) {
                let Some(need) = requirement(objective) else { continue };
                let remaining = objective.count.unwrap_or(0).saturating_sub(status.submitted);
                if status.done || remaining == 0 {
                    continue;
                }
                let use_in_dungeon = objective.kind == "Use Item";
                let key = format!(
                    "{}|{}|{}|{}",
                    if use_in_dungeon { "use" } else { "bring" },
                    need.item_id.unwrap_or_default(),
                    need.item_type.unwrap_or_default(),
                    need.min_rarity.map_or("", game_data::Rarity::name),
                );
                let slot = *by_key.entry(key.clone()).or_insert_with(|| {
                    needs.push(ItemNeed {
                        key,
                        use_in_dungeon,
                        item: objective_item(ctx, &need),
                        min_rarity: need.min_rarity.map(game_data::Rarity::name),
                        owned: ctx.owned.count(&need, ctx.stash_label),
                        quests: Vec::new(),
                    });
                    needs.len() - 1
                });
                needs[slot].quests.push(NeedQuest {
                    quest_id: entry.quest.id.clone(),
                    title: entry.quest.title.clone(),
                    merchant: entry.merchant.clone(),
                    remaining,
                    state: entry.state,
                });
            }
        }
    }
    needs.sort_by(|a, b| {
        a.item.name.to_lowercase().cmp(&b.item.name.to_lowercase()).then_with(|| a.key.cmp(&b.key))
    });
    needs
}
