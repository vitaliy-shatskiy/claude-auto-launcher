//! The launch rows, the launch state and the claude arguments it produces (`Screens.ps1` top half
//! and `LaunchModes.ps1`). Pure: no file reads, no console.

pub mod frame;
pub mod screen;

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::config::Account;
use crate::theme;
use crate::types::DefaultLabels;

/// The rows of the launch screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowName {
    Account,
    Remote,
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
    let mut model = RowDef::plain(RowName::Model, "model", &["default", "fable", "opus1m", "sonnet1m", "haiku"]);
    model.labels = pairs(&[
        ("fable", "Fable 5.1"),
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

/// One launch mode (`LaunchModes.ps1`): the project screen's action field values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchMode {
    pub name: &'static str,
    pub hotkey: Option<char>,
    pub flag: &'static str,
    pub attaches_newest: bool,
    pub picks_session: bool,
    /// Why the mode cannot launch through crc; empty when it can.
    pub remote_off: &'static str,
}

const CANNOT_ATTACH: &str = "crc births a new session, it cannot attach to an existing one";

pub const LAUNCH_MODES: [LaunchMode; 4] = [
    LaunchMode {
        name: "new",
        hotkey: None,
        flag: "",
        attaches_newest: false,
        picks_session: false,
        remote_off: "",
    },
    LaunchMode {
        name: "continue",
        hotkey: Some('c'),
        flag: "-c",
        attaches_newest: true,
        picks_session: false,
        remote_off: CANNOT_ATTACH,
    },
    LaunchMode {
        name: "resume",
        hotkey: Some('r'),
        flag: "--resume",
        attaches_newest: false,
        picks_session: true,
        remote_off: CANNOT_ATTACH,
    },
    LaunchMode {
        name: "worktree",
        hotkey: Some('t'),
        flag: "-w",
        attaches_newest: false,
        picks_session: false,
        remote_off: "a worktree launch is not routed through crc (unverified interaction)",
    },
];

/// Keys the project screen keeps for itself: a mode hotkey on one would replace it.
pub const PROJECT_SCREEN_OWN_KEYS: [char; 5] = ['/', 'w', 'a', 's', 'd'];

/// `Get-LaunchMode`: case-insensitive, like the switch it replaced.
pub fn launch_mode(name: &str) -> Option<&'static LaunchMode> {
    LAUNCH_MODES.iter().find(|m| m.name.eq_ignore_ascii_case(name))
}

/// `Get-LaunchModeTableErrors`: every hotkey clash, one line each.
pub fn launch_mode_table_errors(modes: &[LaunchMode], reserved: &[char]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen: Vec<(char, &str)> = Vec::new();
    for m in modes {
        let Some(k) = m.hotkey else { continue };
        if reserved.contains(&k) {
            errors.push(format!(
                "mode '{}': hotkey '{k}' is a project-screen key (/ filter, w/s move, a/d action)",
                m.name
            ));
        }
        if let Some((_, owner)) = seen.iter().find(|(c, _)| *c == k) {
            errors.push(format!("mode '{}': hotkey '{k}' is already mode '{owner}'s", m.name));
        } else {
            seen.push((k, m.name));
        }
    }
    errors
}

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

/// The model row key a 'default' launch pins on an account lacking the default's family (CS-231).
const UNAVAILABLE_DEFAULT_KEY: &str = "opus1m";

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
    pub fn unavailable_default_model(&self) -> (String, String) {
        let row = self.row(RowName::Model);
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
        }
    }

    pub fn get(&self, row: RowName) -> &str {
        match row {
            RowName::Account => &self.account,
            RowName::Remote => &self.remote,
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

/// `Get-LaunchArgs`: a default state yields NOTHING. `available`/`default_model`: the one
/// exception, the CS-231 pin when the account lacks the default model's family.
pub fn launch_args(
    roster: &Roster,
    state: &LaunchState,
    resume_id: Option<&str>,
    fork: bool,
    available: Option<&[String]>,
    default_model: &str,
) -> Vec<String> {
    let mut a = Vec::new();
    let model_row = roster.row(RowName::Model);
    let mut model_arg = model_row
        .and_then(|r| r.args.iter().find(|(k, _)| k.eq_ignore_ascii_case(&state.model)))
        .map(|(_, v)| v.clone());
    if model_arg.is_none()
        && state.model.eq_ignore_ascii_case("default")
        && !default_model.is_empty()
        && default_model_unavailable(default_model, available)
    {
        model_arg = Some(roster.unavailable_default_model().0).filter(|s| !s.is_empty());
    }
    if let Some(m) = model_arg {
        a.extend(["--model".to_owned(), m]);
    }
    if !state.effort.eq_ignore_ascii_case("default") {
        a.extend(["--effort".to_owned(), state.effort.clone()]);
    }
    if ["fable", "opus"].iter().any(|v| v.eq_ignore_ascii_case(&state.advisor)) {
        a.extend(["--advisor".to_owned(), state.advisor.clone()]);
    }
    if !state.permission.eq_ignore_ascii_case("default") {
        let mode = if state.permission.eq_ignore_ascii_case("bypass") {
            "bypassPermissions".to_owned()
        } else {
            state.permission.clone()
        };
        a.extend(["--permission-mode".to_owned(), mode]);
    }
    if state.mode.eq_ignore_ascii_case("safe") {
        a.push("--safe-mode".into());
    }
    if let Some(m) = launch_mode(&state.action).filter(|m| !m.flag.is_empty()) {
        if !m.picks_session {
            a.push(m.flag.into());
        } else if let Some(id) = resume_id.filter(|s| !s.is_empty()) {
            a.extend([m.flag.to_owned(), id.to_owned()]);
            if fork {
                a.push("--fork-session".into());
            }
        }
    }
    a
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn four() -> Roster {
        let acct = |k: &str, t: &str, hidden: bool| Account {
            key: k.into(),
            root: format!(r"C:\r\{k}"),
            label: format!("{k} account"),
            tint: t.into(),
            hidden,
            canonical: k == "work",
        };
        Roster::new(
            &[
                acct("work", "Green", false),
                acct("personal", "Magenta", false),
                acct("shared", "Cyan", true),
                acct("low", "Blue", false),
            ],
            true,
            None,
        )
    }

    fn args(r: &Roster, f: impl FnOnce(&mut LaunchState)) -> String {
        let mut s = r.new_state();
        f(&mut s);
        launch_args(r, &s, None, false, None, "").join(" ")
    }

    #[test]
    fn row_order_is_account_remote_model_effort_advisor_permission_mode() {
        let names: Vec<&str> = four().rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.join(","), "Account,Remote,Model,Effort,Advisor,Permission,Mode");
        assert_eq!(
            four().row(RowName::Account).map(|r| r.values.join(",")),
            Some("work,personal,low".into())
        );
    }

    #[test]
    fn default_state_adds_no_arguments() {
        assert_eq!(args(&four(), |_| {}), "");
    }

    #[test]
    fn each_row_maps_to_its_flag() {
        let r = four();
        assert_eq!(args(&r, |s| s.model = "opus1m".into()), "--model opus[1m]");
        assert_eq!(args(&r, |s| s.model = "sonnet1m".into()), "--model sonnet[1m]");
        assert_eq!(args(&r, |s| s.permission = "bypass".into()), "--permission-mode bypassPermissions");
        assert_eq!(args(&r, |s| s.advisor = "off".into()), "", "off is an environment variable");
        assert_eq!(args(&r, |s| s.action = "continue".into()), "-c");
        assert_eq!(args(&r, |s| s.action = "worktree".into()), "-w");
        assert_eq!(args(&r, |s| s.action = "resume".into()), "", "resume without an id adds nothing");
        assert_eq!(
            args(&r, |s| {
                s.model = "opus1m".into();
                s.effort = "ultracode".into();
                s.advisor = "fable".into();
                s.permission = "plan".into();
                s.mode = "safe".into();
            }),
            "--model opus[1m] --effort ultracode --advisor fable --permission-mode plan --safe-mode"
        );
    }

    #[test]
    fn resume_carries_id_and_fork() {
        let r = four();
        let mut s = r.new_state();
        s.action = "resume".into();
        assert_eq!(
            launch_args(&r, &s, Some("abc123"), true, None, "").join(" "),
            "--resume abc123 --fork-session"
        );
    }

    #[test]
    fn default_pins_opus_when_account_lacks_fable() {
        let r = four();
        let s = r.new_state();
        let none: Vec<String> = Vec::new();
        assert_eq!(
            launch_args(&r, &s, None, false, Some(&none), "Fable 5.1[1M]").join(" "),
            "--model opus[1m]"
        );
        assert_eq!(
            launch_args(&r, &s, None, false, None, "Fable 5.1[1M]").join(" "),
            "",
            "no information pins nothing"
        );
        let fable = vec!["fable".to_owned()];
        assert_eq!(launch_args(&r, &s, None, false, Some(&fable), "Fable 5.1[1M]").join(" "), "");
    }

    #[test]
    fn step_wraps_and_skips_hidden_fable() {
        let r = four();
        let mut s = r.new_state();
        s.row = 2;
        let none: Vec<String> = Vec::new();
        let labels = DefaultLabels::new();
        let ctx = OptionText::plain(Some(&none), &labels);
        step_launch_value(&r, &mut s, 1, &ctx);
        assert_eq!(s.model, "opus1m", "fable is hidden on this account");
        step_launch_value(&r, &mut s, -2, &ctx);
        assert_eq!(s.model, "haiku", "wraps backwards");
    }

    #[test]
    fn folded_twin_takes_the_default_slot_and_stepping_onto_it_stores_the_twin() {
        let r = four();
        let labels: DefaultLabels = [("Effort".to_owned(), "high".to_owned())].into();
        let ctx = OptionText::plain(None, &labels);
        let row = r.row(RowName::Effort).unwrap();
        assert_eq!(
            visible_row_values(&r, row, &ctx).join(" "),
            "low medium default xhigh max ultracode"
        );
        let mut s = r.new_state();
        s.row = 3;
        s.effort = "medium".into();
        step_launch_value(&r, &mut s, 1, &ctx);
        assert_eq!(s.effort, "high");
        assert!(is_hidden_row_value(&r, row, "high", &ctx));
    }

    #[test]
    fn snap_resets_hidden_family_and_its_mark() {
        let mut s = four().new_state();
        s.model = "fable".into();
        s.restored = vec!["Model".into(), "Effort".into()];
        availability_snap(&mut s, Some(&[]));
        assert_eq!(s.model, "default");
        assert_eq!(s.restored, ["Effort"]);
    }

    #[test]
    fn honest_label_names_the_pin() {
        let r = four();
        assert_eq!(
            honest_default_label(&r, "default (Fable 5.1[1M])", Some(&[]), RowName::Model),
            "default (Opus 5.5[1M])"
        );
        assert_eq!(
            honest_default_label(&r, "fable", Some(&[]), RowName::Advisor),
            "default (plan default)"
        );
    }

    #[test]
    fn launch_mode_table_is_valid_and_case_insensitive() {
        assert_eq!(
            launch_mode_table_errors(&LAUNCH_MODES, &PROJECT_SCREEN_OWN_KEYS),
            [] as [std::string::String; 0]
        );
        assert_eq!(launch_mode("RESUME").map(|m| m.flag), Some("--resume"));
        let clash = [LaunchMode {
            hotkey: Some('w'),
            ..LAUNCH_MODES[1]
        }];
        assert_eq!(launch_mode_table_errors(&clash, &PROJECT_SCREEN_OWN_KEYS).len(), 1);
    }

    #[test]
    fn model_labels_from_catalog_keep_sonnet_bracketless() {
        let mut r = four();
        r.set_model_labels(
            &[
                ("opus".to_owned(), "Opus 6".to_owned()),
                ("sonnet".to_owned(), "Sonnet 6".to_owned()),
            ]
            .into(),
        );
        let row = r.row(RowName::Model).unwrap();
        assert_eq!(row.labels.get("opus1m").map(String::as_str), Some("Opus 6[1M]"));
        assert_eq!(row.labels.get("sonnet1m").map(String::as_str), Some("Sonnet 6"));
    }
}
