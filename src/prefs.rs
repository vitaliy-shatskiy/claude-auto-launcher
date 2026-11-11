//! Remembered launch choices, one profile per account (`Prefs.ps1`, file v2): read with v1
//! migration, save under a per-file mutex with an atomic temp+rename write (UTF-8, no BOM),
//! merge onto a fresh state, and the per-tab switch and reset of the launch screen.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::config::PathCtx;
use crate::launch::{LaunchState, PROFILE_FIELDS, Roster, RowName, Stash};
use crate::sys;

/// The prefs file as read. Always the v2 shape: migration happens on the way in.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prefs {
    pub account: Option<String>,
    pub saved_at_ms: Option<i64>,
    /// Account -> its profile entry (Model, Effort, Advisor, Permission, Remote, Project, `SavedAtMs`).
    pub profiles: Map<String, Value>,
    /// The file exists but could not be read: saving would delete profiles this process cannot see.
    pub unreadable: bool,
}

/// `Get-LaunchPrefsPath`: `CLAUDE_AUTO_PREFS` overrides.
pub fn prefs_path(ctx: &PathCtx, env_override: Option<&str>) -> String {
    match env_override {
        Some(v) if !v.is_empty() => ctx.expand(v),
        _ => format!("{}\\.claude\\claude-auto-prefs.json", ctx.home.to_string_lossy()),
    }
}

fn as_long(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64().map(|f| {
                #[expect(clippy::cast_possible_truncation, reason = "[long] cast of a JSON number")]
                let r = f.round() as i64;
                r
            })
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Parses the file text; a corrupt file reads as empty (it is rewritten on the next save).
pub fn parse_prefs(text: &str) -> Prefs {
    let empty = Prefs::default();
    let Ok(Value::Object(flat)) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
        return empty;
    };
    if flat.is_empty() {
        return empty;
    }
    let version = match flat.get("Version") {
        None => 1,
        Some(v) => match as_long(v) {
            Some(n) => n,
            None => return empty,
        },
    };
    if version > 2 {
        return empty;
    }
    let saved_at_ms = match flat.get("SavedAtMs") {
        None => None,
        Some(v) => match as_long(v) {
            Some(n) => Some(n),
            None => return empty,
        },
    };
    let account = flat
        .get("Account")
        .filter(|v| crate::config::ps_truthy(Some(v)))
        .map(crate::config::ps_str);
    if version >= 2 {
        let profiles = match flat.get("Profiles") {
            Some(Value::Object(p)) => p
                .iter()
                .map(|(k, v)| (k.clone(), if v.is_object() { v.clone() } else { json!({}) }))
                .collect(),
            _ => Map::new(),
        };
        return Prefs {
            account,
            saved_at_ms,
            profiles,
            unreadable: false,
        };
    }
    // v1: the flat fields belong to whichever account the file remembers; none = work.
    let account = account.unwrap_or_else(|| "work".into());
    let mut entry = Map::new();
    for f in PROFILE_FIELDS {
        if let Some(v) = flat.get(f.as_str()).filter(|v| !v.is_null()) {
            entry.insert(f.as_str().into(), v.clone());
        }
    }
    let mut profiles = Map::new();
    if !entry.is_empty() {
        if let Some(ms) = saved_at_ms {
            entry.insert("SavedAtMs".into(), json!(ms));
        }
        profiles.insert(account.clone(), Value::Object(entry));
    }
    Prefs {
        account: Some(account),
        saved_at_ms,
        profiles,
        unreadable: false,
    }
}

/// `Read-LaunchPrefs`: missing = empty; unreadable = empty AND marked, so save will not rebuild.
pub fn read_prefs(path: &str) -> Prefs {
    if !Path::new(path).exists() {
        return Prefs::default();
    }
    match std::fs::read(path) {
        Ok(bytes) => parse_prefs(&String::from_utf8_lossy(&bytes)),
        Err(_) => Prefs {
            unreadable: true,
            ..Prefs::default()
        },
    }
}

/// `Get-PrefsAgeText`: epoch milliseconds to 'just now', 'N min', 'N h', 'N d'.
pub fn age_text(saved_at_ms: Option<i64>, now_ms: i64) -> String {
    let Some(saved) = saved_at_ms else { return String::new() };
    #[expect(clippy::cast_precision_loss, reason = "a millisecond span as minutes")]
    let minutes = now_ms.saturating_sub(saved) as f64 / 60_000.0;
    let n0 = |x: f64| format!("{:.0}", x.round());
    if minutes < 1.0 {
        "just now".into()
    } else if minutes < 60.0 {
        format!("{} min", n0(minutes))
    } else if minutes < 1440.0 {
        format!("{} h", n0(minutes / 60.0))
    } else {
        format!("{} d", n0(minutes / 1440.0))
    }
}

fn entry_str(entry: &Map<String, Value>, key: &str) -> Option<String> {
    entry.get(key).filter(|v| !v.is_null()).map(crate::config::ps_str)
}

/// The file's profile entry for `account`, as a map.
fn profile<'a>(prefs: &'a Prefs, account: &str) -> Option<&'a Map<String, Value>> {
    prefs.profiles.get(account).and_then(Value::as_object)
}

/// `Save-LaunchPrefs`'s merge: this account's entry rebuilt, every other profile copied forward.
pub fn build_prefs_data(prev: &Prefs, state: &LaunchState, now_ms: i64, roster: &Roster) -> Value {
    let account = if state.account.is_empty() {
        "work".to_owned()
    } else {
        state.account.clone()
    };
    let empty = Map::new();
    let prev_entry = profile(prev, &account).unwrap_or(&empty);
    let mut entry = Map::new();
    entry.insert("SavedAtMs".into(), json!(now_ms));
    for f in PROFILE_FIELDS {
        let key = f.as_str();
        if roster.row(f).is_none() {
            if let Some(v) = prev_entry.get(key) {
                entry.insert(key.into(), v.clone());
            }
            continue;
        }
        let v = state.get(f);
        if f == RowName::Remote && v.eq_ignore_ascii_case("stop server") {
            entry.insert(key.into(), json!("off"));
            continue;
        }
        if v.eq_ignore_ascii_case("default") {
            if let Some(p) = prev_entry
                .get(key)
                .filter(|p| !crate::config::ps_str(p).eq_ignore_ascii_case("default"))
            {
                entry.insert(key.into(), p.clone());
            }
            continue;
        }
        if !v.is_empty() {
            entry.insert(key.into(), json!(v));
        }
    }
    if !state.project.is_empty() {
        entry.insert("Project".into(), json!(state.project));
    } else if let Some(p) = prev_entry.get("Project") {
        entry.insert("Project".into(), p.clone());
    }
    let mut profiles = prev.profiles.clone();
    profiles.insert(account.clone(), Value::Object(entry));
    json!({ "Version": 2, "Account": account, "SavedAtMs": now_ms, "Profiles": profiles })
}

/// `Get-PrefsMutexName`: one mutex per prefs FILE, named by a hash of its full lower-case path.
pub fn mutex_name(path: &str) -> String {
    let norm = std::path::absolute(path).map_or_else(|_| path.to_lowercase(), |p| p.to_string_lossy().to_lowercase());
    let hex = sys::sha256(norm.as_bytes()).map_or_else(String::new, |h| hex(h.get(..16).unwrap_or_default()));
    format!("Local\\claude-auto-prefs-{hex}")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| [DIGITS[usize::from(b >> 4)], DIGITS[usize::from(b & 15)]])
        .map(char::from)
        .collect()
}

/// `Save-LaunchPrefs`: read, merge and write under one lock. A lock that does not arrive in 2 s,
/// or an unreadable file, is a skipped write, never a delay or a loss. Returns what was built.
pub fn save_prefs(path: &str, state: &LaunchState, now_ms: i64, roster: &Roster) -> Value {
    let lock = sys::NamedMutex::acquire(&mutex_name(path), 2000);
    let prev = read_prefs(path);
    let data = build_prefs_data(&prev, state, now_ms, roster);
    if prev.unreadable || !lock.as_ref().is_some_and(sys::NamedMutex::held) {
        return data;
    }
    if write_atomic(path, &data).is_err() {
        // Never let a preferences write stop a session from starting.
    }
    data
}

pub(crate) fn write_atomic(path: &str, data: &Value) -> std::io::Result<()> {
    let p = Path::new(path);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!(
        "{path}.{}-{}.tmp",
        std::process::id(),
        crate::log::new_run_id().get(..6).unwrap_or_default()
    );
    let text = serde_json::to_string_pretty(data).map_err(std::io::Error::other)?;
    let result = std::fs::write(&tmp, text.as_bytes()).and_then(|()| std::fs::rename(&tmp, p));
    if result.is_err() && std::fs::remove_file(&tmp).is_err() {
        // The temp may never have been created.
    }
    result
}

/// What `Merge-LaunchPrefs` applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Merged {
    pub restored: Vec<String>,
    pub age_text: String,
}

fn is_dir(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_dir()
}

/// Applies `entry`'s habit rows that are one of the row's own options (case-insensitive, stored in
/// the row's casing); returns the rows applied.
fn apply_entry(state: &mut LaunchState, entry: &Map<String, Value>, roster: &Roster) -> Vec<String> {
    let mut restored = Vec::new();
    for f in PROFILE_FIELDS {
        let Some(v) = entry_str(entry, f.as_str()) else { continue };
        let Some(canon) = roster.row(f).and_then(|r| r.canonical(&v)) else {
            continue;
        };
        state.set(f, canon);
        restored.push(f.as_str().to_owned());
    }
    restored
}

/// `Merge-LaunchPrefs`: the remembered account (validated against the account row) and its
/// profile onto a fresh state; the project only when it is an existing directory.
pub fn merge_prefs(state: &mut LaunchState, prefs: &Prefs, roster: &Roster, now_ms: i64) -> Merged {
    let mut restored = Vec::new();
    if let Some(a) = prefs.account.as_deref()
        && let Some(canon) = roster.row(RowName::Account).and_then(|r| r.canonical(a))
    {
        canon.clone_into(&mut state.account);
        restored.push("Account".to_owned());
    }
    let mut age = String::new();
    if let Some(entry) = profile(prefs, &state.account) {
        restored.extend(apply_entry(state, entry, roster));
        let rp = entry_str(entry, "Project").unwrap_or_default();
        if is_dir(&rp) {
            state.project = rp;
        }
        age = age_text(entry.get("SavedAtMs").and_then(as_long), now_ms);
    }
    if age.is_empty() {
        age = age_text(prefs.saved_at_ms, now_ms);
    }
    state.restored.clone_from(&restored);
    state.restored_age.clone_from(&age);
    Merged { restored, age_text: age }
}

/// `Switch-LaunchAccount`: parks the tab being left in this session's stash, then loads the one
/// arrived at from the stash, else the file, else the defaults.
pub fn switch_account(state: &mut LaunchState, to: &str, prefs: &Prefs, roster: &Roster, now_ms: i64) {
    let mut stash = Stash {
        values: PROFILE_FIELDS.iter().map(|f| (*f, state.get(*f).to_owned())).collect(),
        restored: state.restored.clone(),
        restored_age: state.restored_age.clone(),
        project: state.project.clone(),
        project_slug: state.project_slug.clone(),
    };
    stash.values.retain(|_, v| !v.is_empty());
    state.profiles.insert(state.account.clone(), stash);
    to.clone_into(&mut state.account);
    let fresh = roster.new_state();
    for f in PROFILE_FIELDS {
        state.set(f, fresh.get(f));
    }
    state.restored.clear();
    state.restored_age.clear();
    state.project.clear();
    state.project_slug.clear();
    if let Some(entry) = state.profiles.get(to).cloned() {
        // A row hidden since the stash was taken keeps its pin or default from `fresh`.
        let shown = |f: RowName| roster.row(f).is_some();
        for (f, v) in entry.values.iter().filter(|(f, _)| shown(**f)) {
            state.set(*f, v);
        }
        state.restored = entry.restored;
        state.restored.retain(|r| RowName::parse(r).is_none_or(shown));
        state.restored_age = entry.restored_age;
        if is_dir(&entry.project) {
            state.project = entry.project;
            state.project_slug = entry.project_slug;
        }
        crate::projects::update_start_selection(state);
        return;
    }
    if let Some(entry) = profile(prefs, to) {
        state.restored = apply_entry(state, entry, roster);
        state.restored_age = age_text(entry.get("SavedAtMs").and_then(as_long), now_ms);
        if state.restored_age.is_empty() {
            state.restored_age = age_text(prefs.saved_at_ms, now_ms);
        }
        let rp = entry_str(entry, "Project").unwrap_or_default();
        if is_dir(&rp) {
            state.project = rp;
            state.project_slug = entry_str(entry, "ProjectSlug").unwrap_or_default();
        }
    }
    crate::projects::update_start_selection(state);
}

/// `Reset-LaunchTab`: ctrl+r resets the ACTIVE tab's habit rows for this launch only.
pub fn reset_tab(state: &mut LaunchState, roster: &Roster) {
    let fresh = roster.new_state();
    for f in PROFILE_FIELDS {
        state.set(f, fresh.get(f));
    }
    state.restored.clear();
    state.restored_age.clear();
    let acct = state.account.clone();
    state.profiles.remove(&acct);
}

#[cfg(test)]
mod tests;
