//! The launch rows, the launch state and the claude arguments it produces (`Screens.ps1` top half
//! and `LaunchModes.ps1`). Pure: no file reads, no console.

mod args;
pub mod frame;
mod options;
pub mod screen;

use std::collections::BTreeMap;

use crate::config::Account;
use crate::projects::StartContext;
use crate::theme;

pub use args::{LAUNCH_MODES, LaunchMode, PROJECT_SCREEN_OWN_KEYS, launch_args, launch_mode, launch_mode_table_errors};
pub use options::{
    OptionText, availability_snap, default_model_unavailable, hidden_families, honest_default_label, is_hidden_row_value,
    step_launch_value, step_option, visible_row_values,
};

/// The rows of the launch screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowName {
    Account,
    Remote,
    /// projectScreen onDemand only: what the launch does; never remembered.
    Action,
    Model,
    Effort,
    Advisor,
    Permission,
    Mode,
}

impl RowName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::Remote => "Remote",
            Self::Action => "Action",
            Self::Model => "Model",
            Self::Effort => "Effort",
            Self::Advisor => "Advisor",
            Self::Permission => "Permission",
            Self::Mode => "Mode",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Account,
            Self::Remote,
            Self::Action,
            Self::Model,
            Self::Effort,
            Self::Advisor,
            Self::Permission,
            Self::Mode,
        ]
        .into_iter()
        .find(|r| r.as_str().eq_ignore_ascii_case(s))
    }
}

/// The action row value that opens the project screen (`projectScreen` onDemand).
pub const PROJECT_ACTION: &str = "project";

/// The rows config `rows` may name, in the order the launch screen draws them by default.
pub const CONFIG_ROWS: [RowName; 7] = [
    RowName::Account,
    RowName::Remote,
    RowName::Model,
    RowName::Effort,
    RowName::Advisor,
    RowName::Permission,
    RowName::Mode,
];

/// Model family -> its model row key, in the default order: weakest to strongest.
pub const MODEL_FAMILY_KEYS: [(&str, &str); 4] = [("haiku", "haiku"), ("sonnet", "sonnet1m"), ("opus", "opus1m"), ("fable", "fable")];

/// The habit rows remembered per account (Prefs.ps1 `$ProfileFields`).
pub const PROFILE_FIELDS: [RowName; 5] = [
    RowName::Model,
    RowName::Effort,
    RowName::Advisor,
    RowName::Permission,
    RowName::Remote,
];

/// One option row: internal keys, their screen labels, and the literal `--model` argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowDef {
    pub name: RowName,
    pub label: &'static str,
    pub values: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub args: BTreeMap<String, String>,
}

impl RowDef {
    fn plain(name: RowName, label: &'static str, values: &[&str]) -> Self {
        Self {
            name,
            label,
            values: values.iter().map(|v| (*v).to_owned()).collect(),
            labels: BTreeMap::new(),
            args: BTreeMap::new(),
        }
    }

    /// `-in` against the row's values: case-insensitive.
    pub fn has(&self, v: &str) -> bool {
        self.values.iter().any(|x| x.eq_ignore_ascii_case(v))
    }

    /// The row's own casing of `v`, when it is one of the values.
    pub fn canonical(&self, v: &str) -> Option<&str> {
        self.values.iter().find(|x| x.eq_ignore_ascii_case(v)).map(String::as_str)
    }
}

fn base_rows() -> Vec<RowDef> {
    let pairs = |p: &[(&str, &str)]| {
        p.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>()
    };
    let mut model = RowDef::plain(RowName::Model, "model", &["default"]);
    model.values.extend(MODEL_FAMILY_KEYS.iter().map(|(_, k)| (*k).to_owned()));
    model.labels = pairs(&[
        ("fable", "Fable 5.2"),
        ("opus1m", "Opus 5.5[1M]"),
        ("sonnet1m", "Sonnet 5.5"),
        ("haiku", "Haiku 5.5"),
    ]);
    model.args = pairs(&[
        ("fable", "fable"),
        ("opus1m", "opus[1m]"),
        ("sonnet1m", "sonnet[1m]"),
        ("haiku", "haiku"),
    ]);
    vec![
        RowDef::plain(RowName::Account, "account", &["work"]),
        model,
        RowDef::plain(
            RowName::Effort,
            "effort",
            &["default", "low", "medium", "high", "xhigh", "max", "ultracode"],
        ),
        RowDef::plain(RowName::Advisor, "advisor", &["default", "fable", "opus", "off"]),
        RowDef::plain(
            RowName::Permission,
            "permission",
            &["default", "plan", "auto", "acceptEdits", "bypass"],
        ),
        RowDef::plain(RowName::Mode, "mode", &["normal", "safe"]),
    ]
}

/// The model row key a 'default' launch pins on an account lacking the default's family (CS-231).
const UNAVAILABLE_DEFAULT_KEY: &str = "opus1m";

/// The advisor row value a 'default' launch pins there for the same reason (CS-1044).
const UNAVAILABLE_DEFAULT_ADVISOR: &str = "opus";

/// The account roster and the rows it shapes (`Set-LaunchRoster`).
#[derive(Debug, Clone)]
pub struct Roster {
    pub rows: Vec<RowDef>,
    tints: BTreeMap<String, String>,
    default_account: String,
}

impl Roster {
    /// Visible keys fill the account row; the Remote row exists only when the feature is on; the
    /// bare-Enter account is `default` when visible, else the first visible key.
    pub fn new(accounts: &[Account], remote: bool, default: Option<&str>) -> Self {
        let mut visible: Vec<String> = accounts.iter().filter(|a| !a.hidden).map(|a| a.key.clone()).collect();
        if visible.is_empty() {
            visible.extend(accounts.first().map(|a| a.key.clone()));
        }
        let default_account = match default {
            Some(d) if visible.iter().any(|v| v.eq_ignore_ascii_case(d)) => d.to_owned(),
            _ => visible.first().cloned().unwrap_or_else(|| "work".into()),
        };
        let mut rows = base_rows();
        if let Some(acct) = rows.first_mut() {
            acct.values = visible;
        }
        if remote {
            rows.insert(1, RowDef::plain(RowName::Remote, "remote", &["on", "off", "on+QR", "stop server"]));
        }
        let tints = accounts.iter().map(|a| (a.key.clone(), a.tint.clone())).collect();
        Self {
            rows,
            tints,
            default_account,
        }
    }

    pub fn row(&self, name: RowName) -> Option<&RowDef> {
        self.rows.iter().find(|r| r.name == name)
    }

    pub fn default_account(&self) -> &str {
        &self.default_account
    }

    /// `Get-AccountTint`: one table for both painters; unknown -> Green.
    pub fn tint(&self, account: &str) -> &'static str {
        self.tints.get(account).and_then(|n| theme::palette(n)).unwrap_or(theme::GREEN)
    }

    /// `rows`: the listed rows in that order, every other row gone. A gone row keeps its default
    /// for the launch: prefs restore only rows the roster has. Remote stays absent when it is off.
    pub fn apply_rows(&mut self, order: &[RowName]) {
        let mut rows = std::mem::take(&mut self.rows);
        self.rows = order
            .iter()
            .filter_map(|name| rows.iter().position(|r| r.name == *name).map(|i| rows.remove(i)))
            .collect();
    }

    /// `projectScreen` onDemand: the action row after account (and remote), `new` first. `project`
    /// opens the project screen for that one launch; it adds no claude argument.
    pub fn add_action_row(&mut self) {
        let after = self
            .rows
            .iter()
            .rposition(|r| matches!(r.name, RowName::Account | RowName::Remote))
            .map_or(0, |i| i + 1);
        let values: Vec<&str> = std::iter::once(LAUNCH_MODES[0].name)
            .chain(std::iter::once(PROJECT_ACTION))
            .chain(LAUNCH_MODES.iter().skip(1).map(|m| m.name))
            .collect();
        self.rows.insert(after, RowDef::plain(RowName::Action, "action", &values));
    }

    /// `modelOrder`: the model row lists `default`, then these families in this order; an unlisted
    /// family is no option, so a remembered pick of it falls back to `default`.
    pub fn apply_model_order(&mut self, families: &[String]) {
        let Some(row) = self.rows.iter_mut().find(|r| r.name == RowName::Model) else {
            return;
        };
        row.values = std::iter::once("default".to_owned())
            .chain(families.iter().filter_map(|f| {
                MODEL_FAMILY_KEYS
                    .iter()
                    .find(|(fam, _)| fam.eq_ignore_ascii_case(f))
                    .map(|(_, k)| (*k).to_owned())
            }))
            .collect();
    }

    /// `Set-ModelRowLabels`: family -> display name from the installed binary; the 1M keys
    /// append their bracket (none for Sonnet, whose 1M context is native).
    pub fn set_model_labels(&mut self, family_labels: &BTreeMap<String, String>) {
        let Some(row) = self.rows.iter_mut().find(|r| r.name == RowName::Model) else {
            return;
        };
        for (key, fam) in [("fable", "fable"), ("opus1m", "opus"), ("sonnet1m", "sonnet"), ("haiku", "haiku")] {
            let Some(label) = family_labels.get(fam).filter(|l| !l.is_empty()) else {
                continue;
            };
            let suffix = if key.ends_with("1m") && fam != "sonnet" { "[1M]" } else { "" };
            row.labels.insert(key.to_owned(), format!("{label}{suffix}"));
        }
    }

    /// `New-LaunchState`: pressing Enter at once adds no arguments at all.
    pub fn new_state(&self) -> LaunchState {
        LaunchState::fresh(&self.default_account)
    }

    /// `Get-UnavailableDefaultModel`: (arg, label) of the pin.
    /// A hidden model row still pins: its definition comes from the base rows then.
    pub fn unavailable_default_model(&self) -> (String, String) {
        let base = base_rows();
        let row = self.row(RowName::Model).or_else(|| base.iter().find(|r| r.name == RowName::Model));
        let get = |m: Option<&BTreeMap<String, String>>| m.and_then(|t| t.get(UNAVAILABLE_DEFAULT_KEY)).cloned().unwrap_or_default();
        (get(row.map(|r| &r.args)), get(row.map(|r| &r.labels)))
    }
}

/// This session's stash of one account's habit rows and marks (`Switch-LaunchAccount`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stash {
    pub values: BTreeMap<RowName, String>,
    pub restored: Vec<String>,
    pub restored_age: String,
    pub project: String,
    pub project_slug: String,
}

/// What the launch screen edits and the launch reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchState {
    pub account: String,
    pub model: String,
    pub effort: String,
    pub advisor: String,
    pub permission: String,
    pub remote: String,
    pub action: String,
    pub mode: String,
    pub row: usize,
    /// Footer button under the mouse (-1 none), row (-1 none), option value ('' none).
    pub hover: i32,
    pub hover_row: i32,
    pub hover_value: String,
    pub profiles: BTreeMap<String, Stash>,
    pub restored: Vec<String>,
    pub restored_age: String,
    pub project: String,
    pub project_slug: String,
    pub project_slugs: Vec<String>,
    /// The context `projects::update_start_selection` re-applies the start rule with on a tab
    /// switch (`$script:LaunchStartContext`); `None` until the start selection ran.
    pub start: Option<StartContext>,
}

impl LaunchState {
    pub fn fresh(account: &str) -> Self {
        Self {
            account: account.to_owned(),
            model: "default".into(),
            effort: "default".into(),
            advisor: "default".into(),
            permission: "default".into(),
            remote: "on".into(),
            action: "new".into(),
            mode: "normal".into(),
            row: 0,
            hover: -1,
            hover_row: -1,
            hover_value: String::new(),
            profiles: BTreeMap::new(),
            restored: Vec::new(),
            restored_age: String::new(),
            project: String::new(),
            project_slug: String::new(),
            project_slugs: Vec::new(),
            start: None,
        }
    }

    pub fn get(&self, row: RowName) -> &str {
        match row {
            RowName::Account => &self.account,
            RowName::Remote => &self.remote,
            RowName::Action => &self.action,
            RowName::Model => &self.model,
            RowName::Effort => &self.effort,
            RowName::Advisor => &self.advisor,
            RowName::Permission => &self.permission,
            RowName::Mode => &self.mode,
        }
    }

    pub fn set(&mut self, row: RowName, value: &str) {
        let slot = match row {
            RowName::Account => &mut self.account,
            RowName::Remote => &mut self.remote,
            RowName::Action => &mut self.action,
            RowName::Model => &mut self.model,
            RowName::Effort => &mut self.effort,
            RowName::Advisor => &mut self.advisor,
            RowName::Permission => &mut self.permission,
            RowName::Mode => &mut self.mode,
        };
        value.clone_into(slot);
    }

    pub fn is_restored(&self, row: RowName) -> bool {
        self.restored.iter().any(|r| r.eq_ignore_ascii_case(row.as_str()))
    }
}

#[cfg(test)]
pub(crate) mod tests;
