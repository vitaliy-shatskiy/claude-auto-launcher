//! Launcher configuration (`Config.ps1`): `~/.claude/claude-auto.json` or `CLAUDE_AUTO_CONFIG`.
//! Every validation failure falls back to that key's default and adds a warning; nothing fails.

mod roster;

use std::path::{Path, PathBuf};

use serde_json::Value;

use roster::default_account;
pub use roster::{Account, parse_roster};

/// Distinct on purpose: two accounts sharing a colour is the confusion a tint exists to prevent.
pub const ALLOWED_TINTS: [&str; 6] = ["Green", "Magenta", "Cyan", "Blue", "Yellow", "Red"];
const ALLOWED_RIDER_MODES: [&str; 3] = ["auto", "on", "off"];
const KNOWN_KEYS: [&str; 8] = [
    "accounts",
    "sharing",
    "remote",
    "riderMcp",
    "extraMcpConfigs",
    "secretsRoot",
    "launchHooks",
    "maintenanceActions",
];
const RESERVED_ACTION_KEYS: [&str; 5] = ["u", "r", "d", "m", "p"];

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

/// The whole launcher configuration after validation.
#[derive(Debug, Clone)]
pub struct Config {
    pub accounts: Vec<Account>,
    pub sharing: bool,
    pub remote: bool,
    pub rider_mcp: RiderMcp,
    pub extra_mcp_configs: Vec<String>,
    pub secrets_root: String,
    pub launch_hooks: Vec<String>,
    pub maintenance_actions: Vec<MaintenanceAction>,
    pub warnings: Vec<String>,
    pub path: String,
}

impl Config {
    pub fn defaults(ctx: &PathCtx, path: &str) -> Self {
        let home = ctx.home.to_string_lossy();
        Self {
            accounts: vec![default_account(ctx)],
            sharing: false,
            remote: false,
            rider_mcp: RiderMcp::Auto,
            extra_mcp_configs: vec![format!("{home}\\.claude\\mcp-shared.json")],
            secrets_root: format!("{home}\\.claude-secrets"),
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
    if let Some(v) = present("launchHooks") {
        cfg.launch_hooks = path_list(v, "launchHooks", ctx, &mut cfg.warnings);
    }
    if let Some(v) = present("maintenanceActions") {
        let (actions, w) = parse_actions(v, ctx);
        cfg.maintenance_actions = actions;
        cfg.warnings.extend(w);
    }
    if cfg.sharing && cfg.accounts.len() < 2 {
        cfg.sharing = false;
        cfg.warnings.push("sharing needs at least two accounts; off".into());
    }
    cfg
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

#[cfg(test)]
mod tests;
