//! The sorter's options and the other settings it reads, under DnDTools' own keys wherever
//! DnDTools had one, so a settings file carried over from it keeps working.

use serde::{Deserialize, Serialize};

use appdata::Settings;
use sorter::plan::{default_sort_order, normalize_sort_order, SortDirection, SortDirective, SortField};

pub const PACK_KEY: &str = "stashPackMode";
pub const STACK_KEY: &str = "stashStackMode";
pub const ORDER_KEY: &str = "stashSortOrder";
/// New in this app: bring bag items over into the stash when sorting.
pub const FROM_BAG_KEY: &str = "stashFromBag";
/// New in this app: the player confirmed the automated-input notice once.
pub const RISK_KEY: &str = "sorterRiskAccepted";
/// Drag delay in seconds (DnDTools' default 0.2).
pub const SPEED_KEY: &str = "sortSpeed";
/// One stash id per tab selector, 0 for none.
pub const TAB_MAPPING_KEY: &str = "stashTabMapping";
/// A saved `input::CalibrationOverride`.
pub const CALIBRATION_KEY: &str = "calibrationOverride";
/// "Auto" or "WIDTHxHEIGHT".
pub const RESOLUTION_KEY: &str = "resolution";
/// Sort learning on or off (default on).
pub const LEARNING_KEY: &str = "sortFeedbackSyncEnabled";

const DEFAULT_SPEED_S: f64 = 0.2;
const MAX_SPEED_S: f64 = 1.0;

/// One sort-order entry as the page and the settings file hold it: `{"field": "rarity",
/// "direction": "desc"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderEntry {
    pub field: String,
    pub direction: String,
}

/// What the page lets the player choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortOptions {
    /// Pack tightly (DnDTools' dense pack mode).
    pub pack: bool,
    /// Merge partial stacks of the same item first.
    pub stack: bool,
    /// Bring the bag's items over into the stash.
    pub from_bag: bool,
    /// Fields to sort by, first one first.
    pub order: Vec<OrderEntry>,
}

impl Default for SortOptions {
    fn default() -> Self {
        SortOptions { pack: false, stack: false, from_bag: false, order: to_entries(&default_sort_order()) }
    }
}

impl SortOptions {
    pub fn load(settings: &Settings) -> Self {
        let defaults = SortOptions::default();
        SortOptions {
            pack: settings.get(PACK_KEY).unwrap_or(defaults.pack),
            stack: settings.get(STACK_KEY).unwrap_or(defaults.stack),
            from_bag: settings.get(FROM_BAG_KEY).unwrap_or(defaults.from_bag),
            order: settings.get::<Vec<OrderEntry>>(ORDER_KEY).unwrap_or(defaults.order),
        }
        .cleaned()
    }

    pub fn save(&self, settings: &mut Settings) -> Result<(), String> {
        self.write(settings).map_err(|err| format!("could not save the sorter options: {err}"))
    }

    fn write(&self, settings: &mut Settings) -> Result<(), appdata::Error> {
        settings.set(PACK_KEY, self.pack)?;
        settings.set(STACK_KEY, self.stack)?;
        settings.set(FROM_BAG_KEY, self.from_bag)?;
        settings.set(ORDER_KEY, &self.order)
    }

    /// The order as the planner takes it: unknown fields dropped, each field once, missing ones
    /// appended (Python's `normalize_sort_order`).
    pub fn directives(&self) -> Vec<SortDirective> {
        let parsed: Vec<SortDirective> = self
            .order
            .iter()
            .filter_map(|entry| SortField::parse(&entry.field).map(|field| SortDirective::new(field, SortDirection::parse(&entry.direction))))
            .collect();
        normalize_sort_order(&parsed)
    }

    /// The same options with the order cleaned up as it is saved.
    pub fn cleaned(self) -> Self {
        let order = to_entries(&self.directives());
        SortOptions { order, ..self }
    }
}

fn to_entries(directives: &[SortDirective]) -> Vec<OrderEntry> {
    directives
        .iter()
        .map(|d| OrderEntry {
            field: d.field.as_str().to_string(),
            direction: if d.direction == SortDirection::Asc { "asc" } else { "desc" }.to_string(),
        })
        .collect()
}

/// The drag delay: the `sortSpeed` setting in seconds, kept within 0-1 s.
pub fn move_delay(settings: &Settings) -> std::time::Duration {
    let seconds = settings.get::<f64>(SPEED_KEY).filter(|s| s.is_finite()).unwrap_or(DEFAULT_SPEED_S);
    std::time::Duration::from_secs_f64(seconds.clamp(0.0, MAX_SPEED_S))
}

/// A resolution the player set by hand ("2560x1440"); `None` for "Auto" or anything unreadable.
pub fn saved_resolution(settings: &Settings) -> Option<(u32, u32)> {
    let text = settings.get::<String>(RESOLUTION_KEY)?;
    let (w, h) = text.trim().split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(field: &str, direction: &str) -> OrderEntry {
        OrderEntry { field: field.into(), direction: direction.into() }
    }

    #[test]
    fn an_order_is_cleaned_like_dndtools_did() {
        let options = SortOptions { order: vec![entry("Rarity", "ASC"), entry("bogus", "desc"), entry("rarity", "desc")], ..SortOptions::default() };
        let cleaned = options.cleaned();
        let fields: Vec<&str> = cleaned.order.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["rarity", "width", "height", "slot", "name"]);
        assert_eq!(cleaned.order[0].direction, "asc");
        assert!(cleaned.order[1..].iter().all(|e| e.direction == "desc"));
    }

    #[test]
    fn the_default_order_is_every_field_descending() {
        let fields: Vec<String> = SortOptions::default().order.into_iter().map(|e| e.field).collect();
        assert_eq!(fields, ["width", "height", "slot", "rarity", "name"]);
    }
}
