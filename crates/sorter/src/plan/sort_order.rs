//! Item sort order: the fields a stash can be sorted by, and the comparator built from them. Port
//! of `item.py`'s class-level sort-order machinery (`SORTABLE_FIELDS`, `_parse_sort_directive`,
//! `normalize_sort_order`, `compare_items`).

use std::cmp::Ordering;
use std::collections::HashSet;

use super::item::{rarity_rank, SortItem};

/// A field a stash can be sorted by. Python: `Item.SORTABLE_FIELDS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortField {
    Width,
    Height,
    /// Equipment slot type ("Head", "Chest", ...). Python literally names this field `"slot"`; it
    /// is not the grid position.
    Slot,
    Rarity,
    Name,
}

/// Every sortable field, in Python's declared order (`_DEFAULT_SORTABLE_FIELDS`).
pub const SORTABLE_FIELDS: [SortField; 5] =
    [SortField::Width, SortField::Height, SortField::Slot, SortField::Rarity, SortField::Name];

impl SortField {
    pub fn as_str(self) -> &'static str {
        match self {
            SortField::Width => "width",
            SortField::Height => "height",
            SortField::Slot => "slot",
            SortField::Rarity => "rarity",
            SortField::Name => "name",
        }
    }

    /// Parses one of `SORTABLE_FIELDS`'s names, trimmed and case-insensitive. Python: the
    /// `field_key not in cls.SORTABLE_FIELDS` guard inside `_parse_sort_directive`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "width" => Some(SortField::Width),
            "height" => Some(SortField::Height),
            "slot" => Some(SortField::Slot),
            "rarity" => Some(SortField::Rarity),
            "name" => Some(SortField::Name),
            _ => None,
        }
    }
}

/// Python's `Item.DEFAULT_DIRECTION` is `"desc"`; only the literal string `"asc"` (trimmed,
/// case-insensitive) ever overrides it, so an unrecognized direction is never an error — it is
/// just `Desc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

impl SortDirection {
    /// Port of `Item._normalize_direction`.
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("asc") {
            SortDirection::Asc
        } else {
            SortDirection::Desc
        }
    }
}

/// One "sort by this field, in this direction" instruction. Python passes these around as
/// `{"field": ..., "direction": ...}` dicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SortDirective {
    pub field: SortField,
    pub direction: SortDirection,
}

impl SortDirective {
    pub fn new(field: SortField, direction: SortDirection) -> Self {
        Self { field, direction }
    }

    /// Parses `"field:direction"`, `"field direction"`, or a bare `"field"` (default direction).
    /// Port of the string branch of `Item._parse_sort_directive`. `None` for an unrecognized
    /// field, same as Python silently discarding the directive.
    pub fn parse(spec: &str) -> Option<Self> {
        let text = spec.trim();
        let (field_part, direction_part) = if let Some(split) = text.split_once(':') {
            split
        } else if let Some(split) = text.split_once(char::is_whitespace) {
            split
        } else {
            (text, "")
        };
        let field = SortField::parse(field_part)?;
        Some(Self { field, direction: SortDirection::parse(direction_part) })
    }
}

/// All fields, in their declared order, each descending. Python: `_DEFAULT_SORT_ORDER`.
pub fn default_sort_order() -> Vec<SortDirective> {
    SORTABLE_FIELDS.iter().map(|&field| SortDirective::new(field, SortDirection::Desc)).collect()
}

/// Deduplicates `directives` by field (first occurrence wins) and appends any of
/// `SORTABLE_FIELDS` missing from it, in `SORTABLE_FIELDS` order, each with the default
/// direction. Port of `Item.normalize_sort_order` (its parsing of raw dicts/strings/tuples has no
/// equivalent here: a caller with raw strings parses them first, e.g. via `parse_sort_order`).
pub fn normalize_sort_order(directives: &[SortDirective]) -> Vec<SortDirective> {
    let mut normalized = Vec::with_capacity(SORTABLE_FIELDS.len());
    let mut seen = HashSet::with_capacity(SORTABLE_FIELDS.len());
    for directive in directives {
        if seen.insert(directive.field) {
            normalized.push(*directive);
        }
    }
    for &field in &SORTABLE_FIELDS {
        if seen.insert(field) {
            normalized.push(SortDirective::new(field, SortDirection::Desc));
        }
    }
    normalized
}

/// Parses each spec with [`SortDirective::parse`], drops anything unrecognized, then normalizes.
/// Port of `Item.normalize_sort_order`'s string-list input form (e.g. a persisted setting).
pub fn parse_sort_order(specs: &[&str]) -> Vec<SortDirective> {
    let parsed: Vec<SortDirective> = specs.iter().filter_map(|s| SortDirective::parse(s)).collect();
    normalize_sort_order(&parsed)
}

fn compare_numeric<T: Ord>(left: T, right: T, direction: SortDirection) -> Ordering {
    let ordering = left.cmp(&right);
    if direction == SortDirection::Asc { ordering } else { ordering.reverse() }
}

fn compare_text(left: &str, right: &str, direction: SortDirection) -> Ordering {
    let ordering = left.trim().to_lowercase().cmp(&right.trim().to_lowercase());
    if direction == SortDirection::Asc { ordering } else { ordering.reverse() }
}

/// Orders two items by `directives` in turn, then — regardless of what `directives` covered —
/// falls back to area, then longest side, then rarity, then name (all descending), and finally
/// `Equal` so a stable sort preserves input order instead of an arbitrary tiebreak. Port of
/// `Item.compare_items`.
pub fn compare_items(left: &SortItem, right: &SortItem, directives: &[SortDirective]) -> Ordering {
    for directive in directives {
        let ordering = match directive.field {
            SortField::Width => compare_numeric(left.width, right.width, directive.direction),
            SortField::Height => compare_numeric(left.height, right.height, directive.direction),
            SortField::Slot => compare_text(&left.slot_type, &right.slot_type, directive.direction),
            SortField::Rarity => {
                compare_numeric(rarity_rank(left.rarity), rarity_rank(right.rarity), directive.direction)
            }
            SortField::Name => compare_text(&left.name, &right.name, directive.direction),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }

    compare_numeric(left.area(), right.area(), SortDirection::Desc)
        .then_with(|| compare_numeric(left.longest_side(), right.longest_side(), SortDirection::Desc))
        .then_with(|| compare_numeric(rarity_rank(left.rarity), rarity_rank(right.rarity), SortDirection::Desc))
        .then_with(|| compare_text(&left.name, &right.name, SortDirection::Desc))
}

/// A reusable comparator closure for `slice::sort_by`. Python: `Item.build_sort_comparator`.
pub fn build_comparator(directives: Vec<SortDirective>) -> impl Fn(&SortItem, &SortItem) -> Ordering {
    move |a, b| compare_items(a, b, &directives)
}
