//! What a row shows and how it steps: hidden families, the folded default cell, the wrap
//! stepper (`Screens.ps1` row helpers). Pure.

use std::sync::LazyLock;

use regex::Regex;

use super::{LaunchState, Roster, RowDef, RowName};
use crate::theme;
use crate::types::DefaultLabels;

/// Families that may be hidden at all, and which option belongs to which family, per row.
const HIDEABLE_FAMILIES: [&str; 1] = ["fable"];

fn option_family(row: RowName, key: &str) -> Option<&'static str> {
    let k = key.to_ascii_lowercase();
    match (row, k.as_str()) {
        (RowName::Model | RowName::Advisor, "fable") => Some("fable"),
        (RowName::Model, "opus1m") | (RowName::Advisor, "opus") => Some("opus"),
        (RowName::Model, "sonnet1m") => Some("sonnet"),
        _ => None,
    }
}

fn has_family_map(row: RowName) -> bool {
    matches!(row, RowName::Model | RowName::Advisor)
}

/// `Get-HiddenFamilies`: `None` hides nothing; an answer hides every hideable family not in it.
pub fn hidden_families(available: Option<&[String]>) -> Vec<&'static str> {
    let Some(have) = available else { return Vec::new() };
    HIDEABLE_FAMILIES
        .iter()
        .copied()
        .filter(|f| !have.iter().any(|h| h.eq_ignore_ascii_case(f)))
        .collect()
}

/// `Get-RowOptionText`: the text a row key is drawn as.
#[derive(Debug, Clone, Copy)]
pub struct OptionText<'a> {
    pub default_model_label: &'a str,
    pub default_advisor_label: &'a str,
    pub available: Option<&'a [String]>,
    pub default_labels: &'a DefaultLabels,
}

static EMPTY_LABELS: LazyLock<DefaultLabels> = LazyLock::new(DefaultLabels::new);

impl<'a> OptionText<'a> {
    pub fn plain(available: Option<&'a [String]>, default_labels: &'a DefaultLabels) -> Self {
        Self {
            default_model_label: "default",
            default_advisor_label: "default",
            available,
            default_labels,
        }
    }

    /// The same context without the folding labels (Get-VisibleRowValues' twin test).
    fn unfolded(self) -> Self {
        Self {
            default_labels: &EMPTY_LABELS,
            ..self
        }
    }

    pub fn text(&self, roster: &Roster, row: &RowDef, key: &str) -> String {
        let is_default = key.eq_ignore_ascii_case("default");
        if is_default {
            if let Some(resolved) = self.default_labels.get(row.name.as_str()).filter(|v| !v.is_empty()) {
                if matches!(row.name, RowName::Model | RowName::Advisor) {
                    return honest_default_label(roster, resolved, self.available, row.name);
                }
                return resolved.clone();
            }
            if row.name == RowName::Model {
                return honest_default_label(roster, self.default_model_label, self.available, RowName::Model);
            }
            if row.name == RowName::Advisor {
                return honest_default_label(roster, self.default_advisor_label, self.available, RowName::Advisor);
            }
        }
        row.labels
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map_or_else(|| key.to_owned(), |(_, v)| v.clone())
    }
}

static FAMILY_WORD: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    HIDEABLE_FAMILIES
        .iter()
        .map(|f| (*f, theme::re(&format!(r"(?i)\b{f}\b"))))
        .collect()
});

/// `Test-DefaultModelUnavailable`: the resolved default names a family this account lacks.
pub fn default_model_unavailable(label: &str, available: Option<&[String]>) -> bool {
    let hidden = hidden_families(available);
    FAMILY_WORD.iter().any(|(f, re)| hidden.contains(f) && re.is_match(label))
}

/// `Get-HonestDefaultLabel`.
pub fn honest_default_label(roster: &Roster, label: &str, available: Option<&[String]>, row: RowName) -> String {
    if !default_model_unavailable(label, available) {
        return label.to_owned();
    }
    if row != RowName::Model {
        return "default (plan default)".into();
    }
    let (_, runs) = roster.unavailable_default_model();
    if label.starts_with("default (") {
        format!("default ({runs})")
    } else {
        runs
    }
}

/// `Get-VisibleRowValues`: values minus hidden families, with the option that reads the same as
/// the resolved default folded INTO the default cell, which takes the twin's slot.
pub fn visible_row_values(roster: &Roster, row: &RowDef, ctx: &OptionText<'_>) -> Vec<String> {
    let hidden = hidden_families(ctx.available);
    let values: Vec<String> = if !has_family_map(row.name) || hidden.is_empty() {
        row.values.clone()
    } else {
        row.values
            .iter()
            .filter(|v| option_family(row.name, v).is_none_or(|f| !hidden.contains(&f)))
            .cloned()
            .collect()
    };
    if !ctx.default_labels.contains_key(row.name.as_str()) || !values.iter().any(|v| v.eq_ignore_ascii_case("default")) {
        return values;
    }
    let default_text = ctx.text(roster, row, "default");
    let plain = ctx.unfolded();
    let twins: Vec<&String> = values
        .iter()
        .filter(|v| !v.eq_ignore_ascii_case("default") && plain.text(roster, row, v).to_lowercase() == default_text.to_lowercase())
        .collect();
    if twins.is_empty() {
        return values;
    }
    let mut ordered = Vec::new();
    let mut placed = false;
    for v in &values {
        if v.eq_ignore_ascii_case("default") {
            continue;
        }
        if twins.contains(&v) {
            if !placed {
                ordered.push("default".to_owned());
                placed = true;
            }
            continue;
        }
        ordered.push(v.clone());
    }
    ordered
}

/// `Test-HiddenRowValue`: `key` is a twin folded into the default cell.
pub fn is_hidden_row_value(roster: &Roster, row: &RowDef, key: &str, ctx: &OptionText<'_>) -> bool {
    if key.eq_ignore_ascii_case("default") || !ctx.default_labels.contains_key(row.name.as_str()) {
        return false;
    }
    let has = |vals: &[String]| vals.iter().any(|v| v.eq_ignore_ascii_case(key));
    has(&visible_row_values(roster, row, &ctx.unfolded())) && !has(&visible_row_values(roster, row, ctx))
}

/// `Set-LaunchAvailabilitySnap`: a remembered Model/Advisor of a hidden family goes back to
/// 'default' and loses its restore mark.
pub fn availability_snap(state: &mut LaunchState, available: Option<&[String]>) {
    let hidden = hidden_families(available);
    if hidden.is_empty() {
        return;
    }
    for rn in [RowName::Model, RowName::Advisor] {
        if option_family(rn, state.get(rn)).is_some_and(|f| hidden.contains(&f)) {
            state.set(rn, "default");
            state.restored.retain(|r| !r.eq_ignore_ascii_case(rn.as_str()));
        }
    }
}

/// `Step-Option`: the one wrap-stepper, canonicalised case-insensitively.
pub fn step_option(values: &[String], current: &str, delta: i64) -> String {
    if values.is_empty() {
        return current.to_owned();
    }
    let n = i64::try_from(values.len()).unwrap_or(1);
    let i = values
        .iter()
        .position(|v| v.eq_ignore_ascii_case(current))
        .map_or(0, |p| i64::try_from(p).unwrap_or(0));
    let j = usize::try_from((i + delta).rem_euclid(n)).unwrap_or(0);
    values.get(j).cloned().unwrap_or_default()
}

/// `Step-LaunchValue`: steps the row under the cursor over its VISIBLE values. Landing on a
/// default cell that folded a twin stores the twin, so the pick is saved.
pub fn step_launch_value(roster: &Roster, state: &mut LaunchState, delta: i64, ctx: &OptionText<'_>) {
    let Some(row) = roster.rows.get(state.row) else { return };
    let values = visible_row_values(roster, row, ctx);
    let mut current = state.get(row.name).to_owned();
    if is_hidden_row_value(roster, row, &current, ctx) {
        current = "default".into();
    }
    let mut next = step_option(&values, &current, delta);
    if next.eq_ignore_ascii_case("default")
        && let Some(twin) = row.values.iter().find(|v| is_hidden_row_value(roster, row, v, ctx))
    {
        next.clone_from(twin);
    }
    state.set(row.name, &next);
}
