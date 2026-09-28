//! Which of the game's stash tab selectors opens a stash. Port of `macros.py`'s
//! `STASH_TYPE_TO_TAB_INDEX` lookup, with the character's own stash list as the fallback mapping.

use input::STASH_TAB_COUNT;
use state::stash::BAG;
use state::{grid_size, is_off_limits};

/// Index (0 = topmost) of the stash tab selector that shows `stash_id`, or `None` when the sorter
/// must not click a tab for it.
///
/// `saved_mapping` is the player's own mapping (DnDTools' `stashTabMapping`: one stash id per
/// selector, 0 for none) and wins whenever it names any tab. Without one, the selectors are taken
/// to follow the order the game lists the character's stashes in (`Character::storages`), which
/// agrees with DnDTools' default mapping for the tabs that mapping covers. Either way the run
/// confirms the right stash is on screen before it drags anything, so a wrong guess stops the run
/// instead of sorting another tab.
///
/// Always `None` for the locked seasonal stash: its selector is never clicked.
pub fn tab_index(saved_mapping: Option<&[i64]>, storages: &[u32], stash_id: u32) -> Option<usize> {
    if is_off_limits(stash_id) || !is_stash_tab(stash_id) {
        return None;
    }
    let index = match saved_mapping.filter(|mapping| mapping.iter().any(|&id| id != 0)) {
        Some(mapping) => mapping.iter().position(|&id| id == i64::from(stash_id))?,
        None => {
            let mut tabs: Vec<u32> = Vec::new();
            for &id in storages {
                if is_stash_tab(id) && !tabs.contains(&id) {
                    tabs.push(id);
                }
            }
            tabs.iter().position(|&id| id == stash_id)?
        }
    };
    (index < STASH_TAB_COUNT).then_some(index)
}

/// Stash tabs are the inventories with a stash grid: everything with a grid but the bag.
fn is_stash_tab(inventory_id: u32) -> bool {
    inventory_id != BAG && grid_size(inventory_id).is_some()
}
