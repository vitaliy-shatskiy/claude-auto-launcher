//! The launcher without its screen, for a client that starts sessions on its own (a phone bridge,
//! a script): `--launcher-options` says what the screen would offer, `--launcher-run <json>` takes
//! those choices and joins the launch after the screen. Pure but for one `is_dir`: the reads and the
//! launch are in `app/headless.rs`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::config::Account;
use crate::launch::{
    LAUNCH_MODES, LaunchState, OptionText, Roster, RowDef, RowName, availability_snap, default_model_key, launch_mode, visible_row_values,
};
use crate::prefs::{self, Prefs};
use crate::sessions::valid_session_id;
use crate::types::{DefaultLabels, Limits};

pub const OPTIONS_FLAG: &str = "--launcher-options";
pub const RUN_FLAG: &str = "--launcher-run";

/// What `--launcher-run` was asked, before any of it is checked against the rows. The row fields
/// are named as `--launcher-options` names its rows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RunRequest {
    pub account: String,
    pub project: String,
    pub action: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub advisor: Option<String>,
    pub permission: Option<String>,
    pub mode: Option<String>,
    pub resume_id: Option<String>,
    #[serde(default)]
    pub fork: bool,
    /// Plugin rows by `<plugin>.<row>` (`pluginRows` of `--launcher-options`); unset = its default.
    #[serde(default)]
    pub plugin_rows: BTreeMap<String, String>,
}

impl RunRequest {
    fn row(&self, name: RowName) -> Option<&str> {
        match name {
            RowName::Model => self.model.as_deref(),
            RowName::Effort => self.effort.as_deref(),
            RowName::Advisor => self.advisor.as_deref(),
            RowName::Permission => self.permission.as_deref(),
            RowName::Mode => self.mode.as_deref(),
            RowName::Account | RowName::Remote | RowName::Action | RowName::Plugin(_) => None,
        }
    }
}

/// The arguments after `--launcher-run`: exactly one JSON object. Strict on purpose: the client
/// builds this line, so anything odd is a bug or an attempt, and a launch on a guessed reading is
/// worse than none.
pub fn parse_run_args(args: &[String]) -> Result<RunRequest, String> {
    let [line] = args else {
        return Err(format!("{RUN_FLAG} takes one argument, a JSON object"));
    };
    serde_json::from_str(line).map_err(|e| format!("{RUN_FLAG}: {e}"))
}

static NO_LABELS: std::sync::LazyLock<DefaultLabels> = std::sync::LazyLock::new(DefaultLabels::new);

/// What the row offers this account, a twin folded into `default` included: the screen stores the
/// twin when it lands there, so the twin is a value of the row and the client gets `defaultLabels`.
fn offered(roster: &Roster, row: &RowDef, available: Option<&[String]>) -> Vec<String> {
    visible_row_values(roster, row, &OptionText::plain(available, &NO_LABELS))
}

fn available<'a>(limits: &'a Limits, account: &str) -> Option<&'a [String]> {
    limits.get(account).and_then(|l| l.available_models.as_deref())
}

/// The rows a client sets, in screen order. The account is chosen on its own, the remote row is
/// the PC's switch, and the action row (when `projectScreen` shows one) is the `actions` list.
fn client_rows(roster: &Roster) -> impl Iterator<Item = &RowDef> {
    roster
        .rows
        .iter()
        .filter(|r| !matches!(r.name, RowName::Account | RowName::Remote | RowName::Action | RowName::Plugin(_)))
}

/// The plugin rows with their `<plugin>.<row>` keys and defaults.
fn plugin_rows(roster: &Roster) -> impl Iterator<Item = (&RowDef, &str, &str)> {
    roster.rows.iter().filter_map(|r| match r.name {
        RowName::Plugin(i) => roster.plugin_row(i).map(|(key, default)| (r, key, default)),
        _ => None,
    })
}

/// `pluginRows`: each value must be one the row declares; an unknown key is refused.
fn resolve_plugin_rows(req: &RunRequest, roster: &Roster, state: &mut LaunchState) -> Result<(), String> {
    for (key, value) in &req.plugin_rows {
        let (row, _, _) = plugin_rows(roster)
            .find(|(_, k, _)| k == key)
            .ok_or_else(|| format!("pluginRows: '{key}' is not a plugin row"))?;
        let canon = row
            .canonical(value)
            .ok_or_else(|| format!("pluginRows: '{key}' = '{value}' is not one of {}", row.values.join(", ")))?;
        state.set(row.name, canon);
    }
    Ok(())
}

fn plugin_rows_json(roster: &Roster) -> Vec<Value> {
    plugin_rows(roster)
        .map(|(r, key, default)| json!({ "name": key, "label": r.label, "values": r.values, "default": default }))
        .collect()
}

fn field(name: RowName) -> &'static str {
    match name {
        RowName::Account => "account",
        RowName::Remote => "remote",
        RowName::Action => "action",
        RowName::Model => "model",
        RowName::Effort => "effort",
        RowName::Advisor => "advisor",
        RowName::Permission => "permission",
        RowName::Mode => "mode",
        RowName::Plugin(_) => "plugin",
    }
}

/// One account's remembered rows on a fresh state, the way its tab shows them on the screen.
fn account_state(prefs_file: &Prefs, account: &str, roster: &Roster) -> LaunchState {
    let one = Prefs {
        account: Some(account.to_owned()),
        profiles: prefs_file.profiles.clone(),
        ..Prefs::default()
    };
    let mut state = roster.new_state();
    account.clone_into(&mut state.account);
    prefs::merge_prefs(&mut state, &one, roster, 0);
    state
}

/// Checks the request against what the screen would have allowed on this account and builds the
/// state the launch reads. Every row the screen shows must be given, no other; the rows it does not
/// show (`remote`, a row `rows` set aside) stay as remembered or pinned.
pub fn resolve_run(req: &RunRequest, roster: &Roster, limits: &Limits, prefs_file: &Prefs) -> Result<LaunchState, String> {
    let account = roster
        .row(RowName::Account)
        .and_then(|r| r.canonical(&req.account))
        .ok_or_else(|| format!("account: unknown account '{}'", req.account))?;
    let have = available(limits, account);
    let mut state = account_state(prefs_file, account, roster);
    state.restored.clear();
    for name in [
        RowName::Model,
        RowName::Effort,
        RowName::Advisor,
        RowName::Permission,
        RowName::Mode,
    ] {
        let key = field(name);
        let Some(row) = roster.row(name) else {
            if req.row(name).is_some() {
                return Err(format!("{key}: not a row of the launch screen"));
            }
            continue;
        };
        let value = req.row(name).ok_or_else(|| format!("{key}: required"))?;
        // `default`: the model the screen selects without a pick (`defaultModel`), as Enter launches it.
        if name == RowName::Model && value.eq_ignore_ascii_case("default") {
            state.set(name, "default");
            continue;
        }
        let values = offered(roster, row, have);
        let Some(canon) = values.iter().find(|v| v.eq_ignore_ascii_case(value)) else {
            return Err(format!("{key}: '{value}' is not one of {}", values.join(", ")));
        };
        state.set(name, canon);
    }
    let mode = launch_mode(&req.action).ok_or_else(|| format!("action: '{}' is not one of {}", req.action, action_names().join(", ")))?;
    mode.name.clone_into(&mut state.action);
    match (&req.resume_id, mode.picks_session) {
        (None, true) => return Err(format!("resumeId: required with action {}", mode.name)),
        (Some(_), false) => return Err(format!("resumeId: only with action {}", resume_name())),
        (Some(id), true) if !valid_session_id(id) => return Err(format!("resumeId: '{id}' is not a session id")),
        _ => {}
    }
    if req.fork && !mode.picks_session {
        return Err(format!("fork: only with action {}", resume_name()));
    }
    if !Path::new(&req.project).is_dir() {
        return Err(format!("project: no such directory '{}'", req.project));
    }
    resolve_plugin_rows(req, roster, &mut state)?;
    req.project.clone_into(&mut state.project);
    Ok(state)
}

fn action_names() -> Vec<&'static str> {
    LAUNCH_MODES.iter().map(|m| m.name).collect()
}

fn resume_name() -> &'static str {
    LAUNCH_MODES.iter().find(|m| m.picks_session).map_or("resume", |m| m.name)
}

fn row_json(roster: &Roster, row: &RowDef) -> Value {
    let text = OptionText::plain(None, &NO_LABELS);
    let values: Vec<Value> = offered(roster, row, None)
        .iter()
        .map(|v| json!({ "value": v, "label": text.text(roster, row, v) }))
        .collect();
    json!({ "name": field(row.name), "label": row.label, "values": values })
}

/// Row -> the values of the row this account does not offer.
fn hidden_json(roster: &Roster, have: Option<&[String]>) -> Map<String, Value> {
    client_rows(roster)
        .filter_map(|row| {
            let shown = offered(roster, row, have);
            let mut gone = offered(roster, row, None);
            gone.retain(|v| !shown.contains(v));
            (!gone.is_empty()).then(|| (field(row.name).to_owned(), json!(gone)))
        })
        .collect()
}

/// Row -> `default (what it resolves to)`, through the same text the screen draws, the pin of a
/// lacking family included. A default that resolves to nothing says nothing; a row without a
/// `default` value (model) has none.
fn default_labels_json(roster: &Roster, have: Option<&[String]>, labels: Option<&DefaultLabels>) -> Map<String, Value> {
    let labels = labels.unwrap_or(&NO_LABELS);
    let text = OptionText::plain(have, labels);
    client_rows(roster)
        .filter(|row| row.has("default") && labels.contains_key(row.name.as_str()))
        .filter_map(|row| {
            let resolved = text.text(roster, row, "default");
            (!resolved.eq_ignore_ascii_case("default")).then(|| (field(row.name).to_owned(), json!(format!("default ({resolved})"))))
        })
        .collect()
}

/// The remembered rows as the screen would restore them. A profile with nothing a client can use
/// is no data; `remote` is not among them, it is the PC's.
fn prefs_json(
    roster: &Roster,
    prefs_file: &Prefs,
    account: &str,
    have: Option<&[String]>,
    labels: Option<&DefaultLabels>,
) -> Map<String, Value> {
    let mut state = account_state(prefs_file, account, roster);
    let model = labels.and_then(|l| l.get(crate::env::MODEL_ID)).map_or("", String::as_str);
    availability_snap(roster, &mut state, have, model);
    let rows: Vec<RowName> = client_rows(roster).map(|r| r.name).collect();
    if !rows.iter().any(|r| state.is_restored(*r)) {
        return Map::new();
    }
    rows.iter().map(|r| (field(*r).to_owned(), json!(state.pick(*r)))).collect()
}

/// The model a launch without a pick runs: the row key the screen selects, or the `settings.json`
/// value passed verbatim (no cell selected).
fn default_model_json(roster: &Roster, have: Option<&[String]>, labels: Option<&DefaultLabels>) -> Value {
    let raw = labels.and_then(|l| l.get(crate::env::MODEL_ID)).map_or("", String::as_str);
    json!(default_model_key(roster, raw, have).unwrap_or_else(|| raw.trim().to_owned()))
}

/// The one line `--launcher-options` prints. `labels`: account -> its resolved defaults.
pub fn options_json(
    roster: &Roster,
    accounts: &[Account],
    limits: &Limits,
    labels: &BTreeMap<String, DefaultLabels>,
    prefs_file: &Prefs,
    version: Option<&str>,
) -> Value {
    let on_screen = roster.row(RowName::Account);
    let listed: Vec<Value> = accounts
        .iter()
        .filter(|a| on_screen.is_some_and(|r| r.values.contains(&a.key)))
        .map(|a| {
            let have = available(limits, &a.key);
            json!({
                "key": a.key,
                "label": a.label,
                "tint": a.tint,
                "configDir": a.root,
                "default": a.key == roster.default_account(),
                "limitPct": limits.get(&a.key).and_then(|l| l.five_hour),
                "hidden": hidden_json(roster, have),
                "defaultLabels": default_labels_json(roster, have, labels.get(&a.key)),
                "defaultModel": default_model_json(roster, have, labels.get(&a.key)),
                "prefs": prefs_json(roster, prefs_file, &a.key, have, labels.get(&a.key)),
            })
        })
        .collect();
    let rows: Vec<Value> = client_rows(roster).map(|r| row_json(roster, r)).collect();
    json!({
        "launcher": "claude-auto",
        "version": version,
        "actions": action_names(),
        "rows": rows,
        "pluginRows": plugin_rows_json(roster),
        "accounts": listed,
    })
}

#[cfg(test)]
mod tests;
