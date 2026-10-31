//! Launcher configuration (`Config.ps1`): `~/.claude/claude-auto.json` or `CLAUDE_AUTO_CONFIG`.
//! Every validation failure falls back to that key's default and adds a warning; nothing fails.

use std::path::{Path, PathBuf};

use serde_json::Value;

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

/// One account of the roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub key: String,
    pub root: String,
    pub label: String,
    pub tint: String,
    pub hidden: bool,
    pub canonical: bool,
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

fn canonical_root(ctx: &PathCtx) -> String {
    format!("{}\\.claude", ctx.home.to_string_lossy())
}

fn default_account(ctx: &PathCtx) -> Account {
    Account {
        key: "work".into(),
        root: canonical_root(ctx),
        label: "work account".into(),
        tint: "Green".into(),
        hidden: false,
        canonical: true,
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

struct RawAccount {
    account: Account,
    archived: bool,
    rooted: bool,
    raw_root: String,
}

fn parse_account(a: &Value, ctx: &PathCtx, warnings: &mut Vec<String>) -> RawAccount {
    let field = |n: &str| a.as_object().and_then(|o| o.get(n));
    let key = field("key").map(ps_str).unwrap_or_default().trim().to_owned();
    let raw_root = field("root").map(ps_str).unwrap_or_default();
    let rooted = is_rooted_raw(&raw_root);
    let root = ctx.expand(&raw_root);
    let canon = canonical_root(ctx);
    let canonical = trim_end_sep(&root).eq_ignore_ascii_case(trim_end_sep(&canon));
    // Fail-safe: the key present and anything but a false value means archived.
    let has_archived = a.as_object().is_some_and(|o| o.contains_key("archived"));
    let (mut archived, w) = to_bool(field("archived"), has_archived, &format!("account '{key}': archived"));
    warnings.extend(w);
    if archived && canonical {
        warnings.push(format!(
            "account '{key}': archived is refused for the canonical account ({canon}); kept live"
        ));
        archived = false;
    }
    let tint_raw = if ps_truthy(field("tint")) {
        field("tint").map(ps_str).unwrap_or_default()
    } else {
        "Green".into()
    };
    let tint = if let Some(t) = ALLOWED_TINTS.iter().find(|t| t.eq_ignore_ascii_case(&tint_raw)) {
        (*t).to_owned()
    } else {
        if !archived {
            warnings.push(format!(
                "account '{key}': tint '{tint_raw}' is not one of {}; using Green",
                ALLOWED_TINTS.join(", ")
            ));
        }
        "Green".to_owned()
    };
    let (hidden, hw) = to_bool(field("hidden"), false, &format!("account '{key}': hidden"));
    if !archived {
        warnings.extend(hw);
    }
    let label = if ps_truthy(field("label")) {
        field("label").map(ps_str).unwrap_or_default()
    } else {
        format!("{key} account")
    };
    RawAccount {
        account: Account {
            key,
            root,
            label,
            tint,
            hidden,
            canonical,
        },
        archived,
        rooted,
        raw_root,
    }
}

/// `ConvertTo-LauncherRoster`: any structural failure returns the default roster.
pub fn parse_roster(raw: &Value, ctx: &PathCtx) -> (Vec<Account>, Vec<String>) {
    let mut warnings = Vec::new();
    let items: Vec<&Value> = match raw {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    let parsed: Vec<RawAccount> = items.iter().map(|a| parse_account(a, ctx, &mut warnings)).collect();
    let live: Vec<&RawAccount> = parsed.iter().filter(|p| !p.archived).collect();
    let mut seen_tints: Vec<String> = Vec::new();
    for p in &live {
        let t = &p.account.tint;
        if seen_tints.iter().any(|s| s.eq_ignore_ascii_case(t)) {
            continue;
        }
        let group: Vec<&str> = live
            .iter()
            .filter(|q| q.account.tint.eq_ignore_ascii_case(t))
            .map(|q| q.account.key.as_str())
            .collect();
        if group.len() > 1 {
            warnings.push(format!("accounts {} share tint '{t}'", group.join(", ")));
        }
        seen_tints.push(t.clone());
    }
    if let Some(fatal) = roster_fatal(&parsed, ctx) {
        warnings.push(format!("accounts: {fatal}; using the default roster"));
        return (vec![default_account(ctx)], warnings);
    }
    (parsed.into_iter().filter(|p| !p.archived).map(|p| p.account).collect(), warnings)
}

fn roster_fatal(parsed: &[RawAccount], ctx: &PathCtx) -> Option<String> {
    if parsed.is_empty() {
        return Some("accounts is empty".into());
    }
    let bad_len: Vec<String> = parsed
        .iter()
        .filter(|p| p.account.key.is_empty() || p.account.key.chars().count() > 8)
        .map(|p| format!("'{}'", p.account.key))
        .collect();
    if !bad_len.is_empty() {
        return Some(format!("account key(s) {} must be 1 to 8 characters", bad_len.join(", ")));
    }
    for p in parsed {
        let n = parsed.iter().filter(|q| q.account.key.eq_ignore_ascii_case(&p.account.key)).count();
        if n > 1 {
            return Some(format!(
                "account key '{}' is used {n} times; account keys must be unique",
                p.account.key
            ));
        }
    }
    let bad_root: Vec<String> = parsed
        .iter()
        .filter(|p| !p.rooted)
        .map(|p| format!("'{}': '{}'", p.account.key, p.raw_root))
        .collect();
    if !bad_root.is_empty() {
        return Some(format!("account root(s) {} must be an absolute path", bad_root.join(", ")));
    }
    if parsed.iter().filter(|p| p.account.canonical).count() != 1 {
        let roots: Vec<String> = parsed.iter().map(|p| format!("'{}': '{}'", p.account.key, p.raw_root)).collect();
        return Some(format!(
            "exactly one account must have root {} (the canonical one); accounts have root(s) {}",
            canonical_root(ctx),
            roots.join(", ")
        ));
    }
    None
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
mod tests {
    use super::*;

    fn ctx() -> PathCtx {
        PathCtx {
            home: PathBuf::from(r"C:\Users\someone"),
            cwd: PathBuf::from(r"C:\work"),
        }
    }

    #[test]
    fn expand_resolves_tilde_and_relative_against_cwd() {
        let c = ctx();
        assert_eq!(c.expand("~"), r"C:\Users\someone");
        assert_eq!(c.expand("~/.claude-x/"), r"C:\Users\someone\.claude-x\");
        assert_eq!(c.expand(r"sub\file.json"), r"C:\work\sub\file.json");
        assert_eq!(c.expand("D:/a/../b"), r"D:\b");
    }

    #[test]
    fn quoted_false_is_false_and_garbage_keeps_default_with_warning() {
        let (b, w) = to_bool(Some(&Value::String("FALSE".into())), true, "sharing");
        assert!(!b);
        assert!(w.is_none());
        let (b, w) = to_bool(Some(&Value::String("nope".into())), false, "sharing");
        assert!(!b);
        assert_eq!(w.as_deref(), Some("sharing: 'nope' is not a boolean; using False"));
    }

    #[test]
    fn four_account_fixture_parses_with_hidden_and_tints() {
        let text = include_str!("../tests/fixtures/config-four.json");
        let cfg = parse_config(text, "x", &ctx());
        let keys: Vec<&str> = cfg.accounts.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["work", "personal", "shared", "low"]);
        assert!(cfg.accounts.iter().any(|a| a.key == "shared" && a.hidden));
        assert!(cfg.sharing && cfg.remote);
        assert_eq!(cfg.canonical().map(|a| a.key.as_str()), Some("work"));
        assert_eq!(cfg.maintenance_actions.len(), 1);
        assert!(cfg.maintenance_actions.first().is_some_and(|a| a.confirm_twice));
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
    }

    #[test]
    fn typo_key_warns_with_suggestion() {
        let cfg = parse_config(r#"{"account": []}"#, "x", &ctx());
        assert_eq!(
            cfg.warnings,
            ["unrecognized config key 'account' is ignored; did you mean 'accounts'?"]
        );
    }

    #[test]
    fn no_canonical_account_falls_back_to_default_roster() {
        let cfg = parse_config(r#"{"accounts":[{"key":"a","root":"D:/x"}]}"#, "x", &ctx());
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(cfg.accounts.first().map(|a| a.key.as_str()), Some("work"));
        assert!(
            cfg.warnings
                .iter()
                .any(|w| w.starts_with("accounts: exactly one account must have root"))
        );
    }

    #[test]
    fn relative_root_is_fatal_and_names_the_raw_value() {
        let cfg = parse_config(
            r#"{"accounts":[{"key":"work","root":"~/.claude"},{"key":"b","root":"rel"}]}"#,
            "x",
            &ctx(),
        );
        assert!(
            cfg.warnings
                .iter()
                .any(|w| w == "accounts: account root(s) 'b': 'rel' must be an absolute path; using the default roster")
        );
    }

    #[test]
    fn canonical_root_with_trailing_slash_is_still_canonical() {
        let cfg = parse_config(
            r#"{"accounts":[{"key":"work","root":"~/.claude/"},{"key":"p","root":"~/.p"}]}"#,
            "x",
            &ctx(),
        );
        assert_eq!(cfg.accounts.len(), 2, "{:?}", cfg.warnings);
    }

    #[test]
    fn archived_account_is_filtered_and_lowercase_tint_canonicalised() {
        let cfg = parse_config(
            r#"{"accounts":[{"key":"work","root":"~/.claude","tint":"magenta"},{"key":"old","root":"~/.o","archived":"yes"}]}"#,
            "x",
            &ctx(),
        );
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(cfg.accounts.first().map(|a| a.tint.as_str()), Some("Magenta"));
    }

    #[test]
    fn bad_json_with_single_backslash_gets_the_hint() {
        let cfg = parse_config(r#"{"secretsRoot": "C:\Users\x"}"#, "p.json", &ctx());
        assert_eq!(cfg.warnings.len(), 1);
        assert!(
            cfg.warnings
                .first()
                .is_some_and(|w| w.contains("a single backslash in a Windows path"))
        );
    }

    #[test]
    fn relative_hook_and_mcp_entries_are_skipped() {
        let cfg = parse_config(
            r#"{"launchHooks":["rel.ps1","C:/abs.ps1"],"extraMcpConfigs":"x.json"}"#,
            "x",
            &ctx(),
        );
        assert_eq!(cfg.launch_hooks, [r"C:\abs.ps1"]);
        assert_eq!(cfg.extra_mcp_configs, [] as [std::string::String; 0]);
        assert!(
            cfg.warnings
                .contains(&"launchHooks: 'rel.ps1' is not an absolute path; skipped".to_owned())
        );
    }

    #[test]
    fn sharing_with_one_account_is_turned_off() {
        let cfg = parse_config(r#"{"sharing":true}"#, "x", &ctx());
        assert!(!cfg.sharing);
        assert_eq!(cfg.warnings, ["sharing needs at least two accounts; off"]);
    }

    #[test]
    fn maintenance_action_rules() {
        let (a, w) = parse_actions(
            &serde_json::json!([{"key":"u","script":"C:/x"},{"key":"i","script":"C:/x"},{"key":"I","script":"C:/y"},{"key":"z","script":"rel"}]),
            &ctx(),
        );
        assert_eq!(a.len(), 1);
        assert_eq!(w.len(), 3);
    }
}
