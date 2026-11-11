//! Launcher configuration (`Config.ps1`): `~/.claude/claude-auto.json` or `CLAUDE_AUTO_CONFIG`.
//! Every validation failure falls back to that key's default and adds a warning; nothing fails.

mod roster;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::launch::{CONFIG_ROWS, MODEL_FAMILY_KEYS, Roster, RowName};
use roster::default_account;
pub use roster::{Account, parse_roster};

/// Distinct on purpose: two accounts sharing a colour is the confusion a tint exists to prevent.
pub const ALLOWED_TINTS: [&str; 6] = ["Green", "Magenta", "Cyan", "Blue", "Yellow", "Red"];
const ALLOWED_RIDER_MODES: [&str; 3] = ["auto", "on", "off"];
const KNOWN_KEYS: [&str; 16] = [
    "accounts",
    "sharing",
    "remote",
    "riderMcp",
    "extraMcpConfigs",
    "secretsRoot",
    "vsecPath",
    "launchHooks",
    "maintenanceActions",
    "rows",
    "hiddenValues",
    "modelOrder",
    "hiddenProjects",
    "hideHeadlessProjects",
    "projectScreen",
    "mouse",
];
/// The maintenance screen's own keys: five built-ins and `l`, the launch-rows editor.
const RESERVED_ACTION_KEYS: [&str; 6] = ["u", "r", "d", "m", "p", "l"];

/// Where `~` and relative paths resolve: the home directory and the launch directory.
#[derive(Debug, Clone)]
pub struct PathCtx {
    pub home: PathBuf,
    pub cwd: PathBuf,
}

impl PathCtx {
    /// `$HOME` is `USERPROFILE` on Windows, as PowerShell reads it.
    pub fn from_env() -> Self {
        let home = std::env::var_os("USERPROFILE").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let cwd = std::env::current_dir().unwrap_or_else(|_| home.clone());
        Self { home, cwd }
    }

    /// `Expand-LauncherPath`: `~` and `~\x` against home, a relative value against `cwd`
    /// explicitly, then a full path (separators normalised, `.`/`..` collapsed).
    pub fn expand(&self, raw: &str) -> String {
        if raw.is_empty() {
            return String::new();
        }
        let joined: PathBuf = if raw == "~" {
            self.home.clone()
        } else if let Some(rest) = raw.strip_prefix("~\\").or_else(|| raw.strip_prefix("~/")) {
            self.home.join(rest)
        } else if is_rooted(raw) {
            PathBuf::from(raw)
        } else {
            self.cwd.join(raw)
        };
        std::path::absolute(&joined).map_or_else(|_| raw.to_owned(), |p| p.to_string_lossy().into_owned())
    }
}

/// `[IO.Path]::IsPathRooted` on Windows: a leading separator or a drive letter and colon.
pub fn is_rooted(s: &str) -> bool {
    let mut c = s.chars();
    match (c.next(), c.next()) {
        (Some('\\' | '/'), _) => true,
        (Some(d), Some(':')) => d.is_ascii_alphabetic(),
        _ => false,
    }
}

/// The rooted test every path key shares, judged on the RAW value before expansion: a full path
/// is always absolute, so the check would never fire after it.
pub fn is_rooted_raw(s: &str) -> bool {
    s == "~" || s.starts_with("~\\") || s.starts_with("~/") || is_rooted(s)
}

fn trim_end_sep(s: &str) -> &str {
    let t = s.trim_end_matches(['\\', '/']);
    // A bare root keeps its separator, as TrimEndingDirectorySeparator does.
    if t.ends_with(':') || t.is_empty() { s } else { t }
}

/// PowerShell's `"$value"` for a JSON value: strings as-is, booleans `True`/`False`, null empty.
pub fn ps_str(v: &Value) -> String {
    match v {
        Value::Null | Value::Object(_) => String::new(),
        Value::Bool(b) => if *b { "True" } else { "False" }.to_owned(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(ps_str).collect::<Vec<_>>().join(" "),
    }
}

/// PowerShell truthiness of a JSON value as a config field reads it.
pub fn ps_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
    }
}

/// `ConvertTo-LauncherBool`: a real boolean or the literal strings true/false
/// (case-insensitive); anything else keeps the default and names the field.
pub fn to_bool(v: Option<&Value>, default: bool, name: &str) -> (bool, Option<String>) {
    match v {
        None | Some(Value::Null) => (default, None),
        Some(Value::Bool(b)) => (*b, None),
        Some(other) => {
            let s = ps_str(other).trim().to_owned();
            if s.eq_ignore_ascii_case("true") {
                (true, None)
            } else if s.eq_ignore_ascii_case("false") {
                (false, None)
            } else {
                let d = if default { "True" } else { "False" };
                (default, Some(format!("{name}: '{s}' is not a boolean; using {d}")))
            }
        }
    }
}

/// A configured maintenance action: one key, a label, an absolute script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceAction {
    pub key: String,
    pub label: String,
    pub script: String,
    pub confirm_twice: bool,
}

/// `riderMcp`: auto, on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiderMcp {
    Auto,
    On,
    Off,
}

/// `projectScreen`: after every launch screen, or only when the launch screen's action asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectScreen {
    Always,
    OnDemand,
}

/// The whole launcher configuration after validation.
#[derive(Debug, Clone)]
#[expect(clippy::struct_excessive_bools, reason = "each bool is one on/off key of the config file")]
pub struct Config {
    /// The launch rows in drawing order, account always among them; `None` = every row.
    pub rows: Option<Vec<RowName>>,
    /// `hiddenValues`: a hidden row's launch value, in the row's own casing; unlisted = default.
    pub hidden_values: BTreeMap<RowName, String>,
    /// Model families in drawing order (lower case); `None` = the default order.
    pub model_order: Option<Vec<String>>,
    /// Projects hidden by hand: full paths, kept when the directory is gone.
    pub hidden_projects: Vec<String>,
    pub hide_headless_projects: bool,
    pub project_screen: ProjectScreen,
    pub mouse: bool,
    pub accounts: Vec<Account>,
    pub sharing: bool,
    pub remote: bool,
    pub rider_mcp: RiderMcp,
    pub extra_mcp_configs: Vec<String>,
    pub secrets_root: String,
    /// `vsec.exe` or its shim; empty = `CLAUDE_AUTO_VSEC`, then PATH.
    pub vsec_path: String,
    /// Top-level entries run at the same time; the paths inside one entry run in order.
    pub launch_hooks: Vec<Vec<String>>,
    pub maintenance_actions: Vec<MaintenanceAction>,
    pub warnings: Vec<String>,
    pub path: String,
}

impl Config {
    pub fn defaults(ctx: &PathCtx, path: &str) -> Self {
        let home = ctx.home.to_string_lossy();
        Self {
            rows: None,
            hidden_values: BTreeMap::new(),
            model_order: None,
            hidden_projects: Vec::new(),
            hide_headless_projects: true,
            project_screen: ProjectScreen::Always,
            mouse: true,
            accounts: vec![default_account(ctx)],
            sharing: false,
            remote: false,
            rider_mcp: RiderMcp::Auto,
            extra_mcp_configs: vec![format!("{home}\\.claude\\mcp-shared.json")],
            secrets_root: format!("{home}\\.claude-secrets"),
            vsec_path: String::new(),
            launch_hooks: Vec::new(),
            maintenance_actions: Vec::new(),
            warnings: Vec::new(),
            path: path.to_owned(),
        }
    }

    /// The canonical account (exactly one after validation).
    pub fn canonical(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.canonical)
    }

    pub fn rows_config(&self) -> RowsConfig {
        RowsConfig {
            rows: self.rows.clone(),
            hidden_values: self.hidden_values.clone(),
            remote: self.remote,
            model_order: self.model_order.clone(),
        }
    }
}

/// What the rows editor edits and the launch screen re-reads: `rows`, `hiddenValues`, and the
/// keys that shape which rows and values exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowsConfig {
    pub rows: Option<Vec<RowName>>,
    pub hidden_values: BTreeMap<RowName, String>,
    pub remote: bool,
    pub model_order: Option<Vec<String>>,
}

/// `Get-LauncherConfigPath`.
pub fn config_path(ctx: &PathCtx, env_override: Option<&str>) -> String {
    match env_override {
        Some(v) if !v.is_empty() => ctx.expand(v),
        _ => format!("{}\\.claude\\claude-auto.json", ctx.home.to_string_lossy()),
    }
}

/// `Get-LauncherNearKey`: a prefix match either way, case-insensitive.
fn near_key(key: &str) -> Option<&'static str> {
    let k = key.to_ascii_lowercase();
    KNOWN_KEYS.iter().copied().find(|known| {
        let kn = known.to_ascii_lowercase();
        kn.starts_with(&k) || k.starts_with(&kn)
    })
}

/// `ConvertTo-MaintenanceActions`.
pub fn parse_actions(raw: &Value, ctx: &PathCtx) -> (Vec<MaintenanceAction>, Vec<String>) {
    let mut out: Vec<MaintenanceAction> = Vec::new();
    let mut warnings = Vec::new();
    let items: Vec<&Value> = match raw {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    for m in items {
        let field = |n: &str| m.as_object().and_then(|o| o.get(n));
        let key = field("key").map(ps_str).unwrap_or_default().trim().to_lowercase();
        let one = key.len() == 1 && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !one || RESERVED_ACTION_KEYS.contains(&key.as_str()) || !ps_truthy(field("script")) {
            warnings.push(format!(
                "maintenanceActions: entry '{key}' needs a one-letter key outside {} and a script; skipped",
                RESERVED_ACTION_KEYS.join(",")
            ));
            continue;
        }
        if out.iter().any(|a| a.key == key) {
            warnings.push(format!(
                "maintenanceActions: duplicate key '{key}'; the first one wins, this entry is skipped"
            ));
            continue;
        }
        let raw_script = field("script").map(ps_str).unwrap_or_default();
        if !is_rooted_raw(&raw_script) {
            warnings.push(format!("maintenanceActions '{key}': script must be an absolute path; skipped"));
            continue;
        }
        let (confirm_twice, w) = to_bool(field("confirmTwice"), false, &format!("maintenanceActions '{key}': confirmTwice"));
        warnings.extend(w);
        let label = if ps_truthy(field("label")) {
            field("label").map(ps_str).unwrap_or_default()
        } else {
            key.clone()
        };
        out.push(MaintenanceAction {
            key,
            label,
            script: ctx.expand(&raw_script),
            confirm_twice,
        });
    }
    (out, warnings)
}

fn path_list(raw: &Value, name: &str, ctx: &PathCtx, warnings: &mut Vec<String>) -> Vec<String> {
    let items: Vec<&Value> = match raw {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    let mut out = Vec::new();
    for e in items {
        let s = ps_str(e);
        if !is_rooted_raw(&s) {
            warnings.push(format!("{name}: '{s}' is not an absolute path; skipped"));
            continue;
        }
        out.push(ctx.expand(&s));
    }
    out
}

/// An ordered list of names out of `known` (case-insensitive, the first of a duplicate wins);
/// unknown names are named once in one warning. `None` = not a list, or no known name in it.
fn name_list<T: Copy + PartialEq>(raw: &Value, key: &str, what: &str, known: &[(&str, T)], warnings: &mut Vec<String>) -> Option<Vec<T>> {
    let names = known.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    let Value::Array(items) = raw else {
        warnings.push(format!("{key} must be a list out of {names}; using the default"));
        return None;
    };
    let mut out = Vec::new();
    let mut unknown = Vec::new();
    for item in items {
        let s = ps_str(item).trim().to_owned();
        match known.iter().find(|(n, _)| n.eq_ignore_ascii_case(&s)) {
            Some((_, v)) if !out.contains(v) => out.push(*v),
            Some(_) => {}
            None => unknown.push(format!("'{s}'")),
        }
    }
    if !unknown.is_empty() {
        warnings.push(format!("{key}: unknown {what} {} ignored (known: {names})", unknown.join(", ")));
    }
    (!out.is_empty()).then_some(out)
}

/// `rows`: account is always drawn, first when unlisted.
fn row_list(raw: &Value, warnings: &mut Vec<String>) -> Option<Vec<RowName>> {
    let lower: Vec<String> = CONFIG_ROWS.iter().map(|r| r.as_str().to_lowercase()).collect();
    let known: Vec<(&str, RowName)> = lower.iter().map(String::as_str).zip(CONFIG_ROWS).collect();
    let mut rows = name_list(raw, "rows", "row name(s)", &known, warnings)?;
    if !rows.contains(&RowName::Account) {
        rows.insert(0, RowName::Account);
    }
    Some(rows)
}

/// `hiddenValues`: row name -> one of that row's values (account excluded); a bad entry warns and
/// is skipped. Checked against the model row as `modelOrder` leaves it.
fn hidden_values(raw: &Value, model_order: Option<&[String]>, warnings: &mut Vec<String>) -> BTreeMap<RowName, String> {
    let mut out = BTreeMap::new();
    let Value::Object(map) = raw else {
        warnings.push("hiddenValues must be an object of row name -> value; ignored".into());
        return out;
    };
    let catalog = Roster::catalog(model_order);
    for (key, v) in map {
        let value = ps_str(v);
        let def = RowName::parse(key)
            .filter(|r| CONFIG_ROWS.contains(r) && *r != RowName::Account)
            .and_then(|r| catalog.def(r));
        let Some(def) = def else {
            warnings.push(format!("hiddenValues: '{key}' is not a row that can be hidden; ignored"));
            continue;
        };
        match def.canonical(&value) {
            Some(c) => {
                out.insert(def.name, c.to_owned());
            }
            None => warnings.push(format!(
                "hiddenValues: '{value}' is not a {key} value ({}); ignored",
                def.values.join(", ")
            )),
        }
    }
    out
}

/// `modelOrder`: family names, stored lower case.
fn family_list(raw: &Value, warnings: &mut Vec<String>) -> Option<Vec<String>> {
    let known: Vec<(&str, &str)> = MODEL_FAMILY_KEYS.iter().map(|(f, _)| (*f, *f)).collect();
    name_list(raw, "modelOrder", "model famil(ies)", &known, warnings).map(|fams| fams.into_iter().map(str::to_owned).collect())
}

/// `launchHooks`: a path is an entry of its own, an array of paths one entry run in order.
fn hook_chains(raw: &Value, ctx: &PathCtx, warnings: &mut Vec<String>) -> Vec<Vec<String>> {
    let Value::Array(items) = raw else {
        return path_list(raw, "launchHooks", ctx, warnings).into_iter().map(|p| vec![p]).collect();
    };
    let mut out = Vec::new();
    for item in items {
        let chain = match item {
            Value::Array(chain) => {
                let paths: Vec<Value> = chain
                    .iter()
                    .filter(|e| {
                        let ok = e.is_string();
                        if !ok {
                            warnings.push(format!(
                                "launchHooks: a hook chain holds only path strings, one level deep; {e} skipped"
                            ));
                        }
                        ok
                    })
                    .cloned()
                    .collect();
                path_list(&Value::Array(paths), "launchHooks", ctx, warnings)
            }
            other => path_list(other, "launchHooks", ctx, warnings),
        };
        if !chain.is_empty() {
            out.push(chain);
        }
    }
    out
}

/// `Read-LauncherConfig` over text already read from `path`.
pub fn parse_config(text: &str, path: &str, ctx: &PathCtx) -> Config {
    let mut cfg = Config::defaults(ctx, path);
    let raw: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let hint = if has_bare_backslash(text) {
                " - a single backslash in a Windows path is the usual cause; use / or double it (\\\\)"
            } else {
                ""
            };
            cfg.warnings = vec![format!("config {path} could not be read or parsed ({e}){hint}; using defaults")];
            return cfg;
        }
    };
    let Some(obj) = raw.as_object() else {
        return cfg;
    };
    for k in obj.keys() {
        if !KNOWN_KEYS.contains(&k.as_str()) {
            let msg = match near_key(k) {
                Some(best) => format!("unrecognized config key '{k}' is ignored; did you mean '{best}'?"),
                None => format!("unrecognized config key '{k}' is ignored (known keys: {})", KNOWN_KEYS.join(", ")),
            };
            cfg.warnings.push(msg);
        }
    }
    let present = |k: &str| obj.get(k).filter(|v| !v.is_null());
    if let Some(a) = obj.get("accounts") {
        if a.is_null() {
            cfg.warnings.push("accounts is null; using the default roster".into());
        } else {
            let (accounts, w) = parse_roster(a, ctx);
            cfg.accounts = accounts;
            cfg.warnings.extend(w);
        }
    }
    if let Some(v) = present("sharing") {
        let (b, w) = to_bool(Some(v), cfg.sharing, "sharing");
        cfg.sharing = b;
        cfg.warnings.extend(w);
    }
    if let Some(v) = present("remote") {
        let (b, w) = to_bool(Some(v), cfg.remote, "remote");
        cfg.remote = b;
        cfg.warnings.extend(w);
    }
    if let Some(v) = present("riderMcp") {
        let mode = ps_str(v).to_lowercase();
        cfg.rider_mcp = match mode.as_str() {
            "auto" => RiderMcp::Auto,
            "on" => RiderMcp::On,
            "off" => RiderMcp::Off,
            _ => {
                cfg.warnings
                    .push(format!("riderMcp '{}' is not auto/on/off; using auto", ps_str(v)));
                RiderMcp::Auto
            }
        };
        debug_assert!(ALLOWED_RIDER_MODES.len() == 3, "the three rider modes");
    }
    if let Some(v) = present("extraMcpConfigs") {
        cfg.extra_mcp_configs = path_list(v, "extraMcpConfigs", ctx, &mut cfg.warnings);
    }
    if let Some(v) = present("secretsRoot") {
        let raw_secrets = ps_str(v).trim().to_owned();
        if raw_secrets.is_empty() {
            cfg.warnings.push(format!("secretsRoot is empty; using {}", cfg.secrets_root));
        } else if !is_rooted_raw(&raw_secrets) {
            cfg.warnings.push("secretsRoot must be an absolute path".into());
        } else {
            cfg.secrets_root = ctx.expand(&raw_secrets);
        }
    }
    if let Some(v) = present("vsecPath") {
        let raw = ps_str(v).trim().to_owned();
        if !raw.is_empty() && !is_rooted_raw(&raw) {
            cfg.warnings
                .push("vsecPath must be an absolute path; looking up vsec instead".into());
        } else if !raw.is_empty() {
            cfg.vsec_path = ctx.expand(&raw);
        }
    }
    if let Some(v) = present("launchHooks") {
        cfg.launch_hooks = hook_chains(v, ctx, &mut cfg.warnings);
    }
    if let Some(v) = present("maintenanceActions") {
        let (actions, w) = parse_actions(v, ctx);
        cfg.maintenance_actions = actions;
        cfg.warnings.extend(w);
    }
    parse_screen_keys(obj, ctx, &mut cfg);
    if cfg.sharing && cfg.accounts.len() < 2 {
        cfg.sharing = false;
        cfg.warnings.push("sharing needs at least two accounts; off".into());
    }
    cfg
}

/// The keys that shape the screens: rows, models, project hiding, the project screen, the mouse.
fn parse_screen_keys(obj: &serde_json::Map<String, Value>, ctx: &PathCtx, cfg: &mut Config) {
    let present = |k: &str| obj.get(k).filter(|v| !v.is_null());
    if let Some(v) = present("rows") {
        cfg.rows = row_list(v, &mut cfg.warnings);
    }
    if let Some(v) = present("modelOrder") {
        cfg.model_order = family_list(v, &mut cfg.warnings);
    }
    if let Some(v) = present("hiddenValues") {
        cfg.hidden_values = hidden_values(v, cfg.model_order.as_deref(), &mut cfg.warnings);
    }
    if let Some(v) = present("hiddenProjects") {
        cfg.hidden_projects = path_list(v, "hiddenProjects", ctx, &mut cfg.warnings);
    }
    if let Some(v) = present("hideHeadlessProjects") {
        let (b, w) = to_bool(Some(v), cfg.hide_headless_projects, "hideHeadlessProjects");
        cfg.hide_headless_projects = b;
        cfg.warnings.extend(w);
    }
    if let Some(v) = present("projectScreen") {
        let raw = ps_str(v);
        cfg.project_screen = if raw.eq_ignore_ascii_case("onDemand") {
            ProjectScreen::OnDemand
        } else {
            if !raw.eq_ignore_ascii_case("always") {
                cfg.warnings
                    .push(format!("projectScreen '{raw}' is not always/onDemand; using always"));
            }
            ProjectScreen::Always
        };
    }
    if let Some(v) = present("mouse") {
        let (b, w) = to_bool(Some(v), cfg.mouse, "mouse");
        cfg.mouse = b;
        cfg.warnings.extend(w);
    }
}

/// `\\[^"\\/bfnrtu]` on the source text: a backslash JSON cannot escape.
fn has_bare_backslash(text: &str) -> bool {
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.peek() {
                Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                    it.next();
                }
                Some(_) => return true,
                None => {}
            }
        }
    }
    false
}

/// `Read-LauncherConfig`: no file = defaults; an unreadable file warns and keeps the defaults.
pub fn read_config(path: &str, ctx: &PathCtx) -> Config {
    if !Path::new(path).exists() {
        return Config::defaults(ctx, path);
    }
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            parse_config(text.trim_start_matches('\u{feff}'), path, ctx)
        }
        Err(e) => {
            let mut cfg = Config::defaults(ctx, path);
            cfg.warnings = vec![format!("config {path} could not be read ({e}); using defaults")];
            cfg
        }
    }
}

/// Writes `keys` into the config file and leaves every other key as it was (order kept).
pub fn save_keys(path: &str, keys: &[(&str, Value)]) -> std::io::Result<()> {
    update_config(path, |obj| {
        for (k, v) in keys {
            obj.insert((*k).to_owned(), v.clone());
        }
    })
}

/// Writes config `rows` and `hiddenValues` (lower-case row names; the key dropped when empty),
/// every other key untouched.
pub fn save_rows(path: &str, rows: &[RowName], hidden: &BTreeMap<RowName, String>) -> std::io::Result<()> {
    let names: Vec<String> = rows.iter().map(|r| r.as_str().to_lowercase()).collect();
    let pins: serde_json::Map<String, Value> = hidden
        .iter()
        .map(|(r, v)| (r.as_str().to_lowercase(), Value::String(v.clone())))
        .collect();
    update_config(path, |obj| {
        obj.insert("rows".into(), serde_json::json!(names));
        if pins.is_empty() {
            obj.shift_remove("hiddenValues");
        } else {
            obj.insert("hiddenValues".into(), Value::Object(pins));
        }
    })
}

/// Rewrites the config through `edit`. A file that is not UTF-8 (an ANSI save would lose its
/// bytes) or not a JSON object is refused, never overwritten; a missing one is created. The write
/// is a temp file renamed over the old one, next to a symlink's target so the link survives: two
/// launchers saving at once leave the last writer's file.
pub(crate) fn update_config(path: &str, edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> std::io::Result<()> {
    // `try_exists`, not `exists`: a file whose metadata fails must never be taken for missing and
    // replaced by `{}`.
    let mut obj = if Path::new(path).try_exists()? {
        let bytes = std::fs::read(path)?;
        let text = std::str::from_utf8(&bytes).map_err(|e| std::io::Error::other(format!("{path} is not UTF-8 ({e})")))?;
        match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).map_err(std::io::Error::other)? {
            Value::Object(m) => m,
            _ => return Err(std::io::Error::other(format!("{path} is not a JSON object"))),
        }
    } else {
        serde_json::Map::new()
    };
    edit(&mut obj);
    let target = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::canonicalize(path)?.to_string_lossy().into_owned()
    } else {
        path.to_owned()
    };
    crate::prefs::write_json(&target, &Value::Object(obj))
}

#[cfg(test)]
mod tests;
