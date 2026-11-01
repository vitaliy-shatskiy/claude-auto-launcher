//! Environment around a launch (`Env.ps1`, `sharing/`), worker W3 (see PORTING.md): profiles
//! and the MCP mirror, secrets, sharing repair (hardlinks, symlinks, junctions, ACL), launch
//! hooks, MCP discovery incl. Rider, the model catalog, rate limits. The entry points here print;
//! the decisions live in `env/*` and return notes.

mod hooks;
mod limits;
mod mcp;
mod mirror;
mod models;
mod secrets;
mod sharing;
mod win_fs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

use crate::config::{Account, Config};
use crate::out::{self, Fg};
use crate::types::{ChildEnv, DefaultLabels, Limits};

pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub(crate) const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// One `Write-Host` line a step wants printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) text: String,
    pub(crate) fg: Fg,
}

impl Note {
    pub(crate) fn new(text: impl Into<String>, fg: Fg) -> Self {
        Self { text: text.into(), fg }
    }
}

fn print(notes: &[Note]) {
    for n in notes {
        out::line(&n.text, n.fg);
    }
}

/// `ConvertTo-SortedJson` / the mirror's `canonical`: keys sorted at every level, array order
/// kept, so key order never decides equality.
pub(crate) fn sorted_json(v: &Value) -> String {
    match v {
        Value::Array(a) => format!("[{}]", a.iter().map(sorted_json).collect::<Vec<_>>().join(",")),
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", Value::String((*k).clone()), sorted_json(&m[k.as_str()])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        other => other.to_string(),
    }
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// `Set-ClaudeProfile` with the mirror injected: the account line, then (sharing on, not preview,
/// a secondary account) the MCP mirror's one line.
fn profile_notes(
    cfg: &Config,
    account: &str,
    preview: bool,
    child: &mut ChildEnv,
    mirror: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Vec<Note>, String> {
    let Some(a) = cfg.accounts.iter().find(|a| a.key == account) else {
        let keys: Vec<&str> = cfg.accounts.iter().map(|a| a.key.as_str()).collect();
        return Err(format!("unknown account '{account}' (roster: {})", keys.join(", ")));
    };
    let mut notes = vec![Note::new(format!("-> {}", a.label), Fg::from_tint(&a.tint))];
    if a.canonical {
        child.remove("CLAUDE_CONFIG_DIR");
        return Ok(notes);
    }
    child.set("CLAUDE_CONFIG_DIR", &a.root);
    // The mirror REWRITES the target's .claude.json: never in preview.
    if !cfg.sharing || preview {
        return Ok(notes);
    }
    match mirror(&a.root) {
        Ok(msg) if !msg.is_empty() => notes.push(Note::new(format!("  MCP mirror: {msg}"), Fg::DarkGray)),
        Ok(_) => {}
        Err(msg) => notes.push(Note::new(format!("  project MCP mirror failed: {msg}"), Fg::DarkYellow)),
    }
    Ok(notes)
}

/// `Set-ClaudeProfile`: the canonical account is the ABSENCE of `CLAUDE_CONFIG_DIR`. Prints the
/// account line in its tint; with sharing on and outside preview, mirrors MCP servers.
pub fn set_profile(cfg: &Config, account: &str, preview: bool, child: &mut ChildEnv) -> Result<(), String> {
    print(&profile_notes(cfg, account, preview, child, &mut mirror_project_mcp)?);
    Ok(())
}

/// The MCP mirror (formerly `sharing/claude-mirror-mcp.mjs` under node) from `~\.claude.json` into
/// `<root>\.claude.json`. `Ok(one-line summary)`, `Err(one-line reason)`.
pub fn mirror_project_mcp(root: &str) -> Result<String, String> {
    let opts = mirror::Options::from_vars(
        std::env::var("CLAUDE_AUTO_MIRROR_EXCLUDE").ok().as_deref(),
        std::env::var("CLAUDE_AUTO_MIRROR_SECRETS").ok().as_deref(),
    );
    mirror::run(
        &home().join(".claude.json"),
        &Path::new(root).join(".claude.json"),
        &mut mirror::RealFs,
        &opts,
    )
}

/// `Import-ProjectSecrets`: one file per variable, tiers shared -> org -> project, reserved shell
/// names refused. Adds to the CHILD environment; returns how many were imported.
pub fn import_project_secrets(cwd: &Path, secrets_root: &str, child: &mut ChildEnv) -> usize {
    let got = secrets::import(&cwd.to_string_lossy(), secrets_root);
    for (k, v) in &got.vars {
        child.set(k, v);
    }
    print(&got.notes);
    got.loaded.len()
}

/// `Set-ClaudeProjectDirectory`: the session starts in the chosen project. A path that is not an
/// existing directory is refused and the launch directory stays.
pub fn set_project_directory(project: &str) -> bool {
    if project.is_empty() {
        return false;
    }
    if !Path::new(project).is_dir() {
        out::line(&format!("  project directory not found: {project} - staying put"), Fg::DarkYellow);
        return false;
    }
    std::env::set_current_dir(project).is_ok()
}

/// The canonical root and every secondary root, from the roster.
fn roots(cfg: &Config) -> Option<(PathBuf, Vec<PathBuf>)> {
    let work = PathBuf::from(&cfg.canonical()?.root);
    Some((
        work,
        cfg.accounts
            .iter()
            .filter(|a| !a.canonical)
            .map(|a| PathBuf::from(&a.root))
            .collect(),
    ))
}

/// `Repair-SharedProfiles`: profile roots, hardlinked files, junctions with their ACL shield, the
/// `CLAUDE.md` symlink. Sharing off or preview: touches nothing.
pub fn repair_shared_profiles(cfg: &Config, preview: bool) {
    if !cfg.sharing || preview {
        return;
    }
    let Some((work, secondary)) = roots(cfg) else { return };
    let mut icacls = sharing::real_icacls;
    let mut seams = sharing::Seams {
        symlink: &sharing::real_symlink,
        icacls: &mut icacls,
    };
    print(&sharing::repair_all(&work, &secondary, sharing::SHARED, &mut seams));
}

/// `Invoke-LaunchHooks`: each configured hook, best-effort, one line on failure.
pub fn run_launch_hooks(hooks: &[Vec<String>]) {
    run_launch_hooks_with_env(hooks, &ChildEnv::default());
}

/// `Invoke-LaunchHooks` with the session's environment (`CLAUDE_CONFIG_DIR`, secrets): the
/// PowerShell launcher set those in its own process, so its hooks saw them.
pub fn run_launch_hooks_with_env(hooks: &[Vec<String>], child: &ChildEnv) {
    print(&hooks::run(hooks, child));
}

/// `Get-McpConfigPaths`: the configured extras that exist, plus the Rider MCP endpoint per
/// `riderMcp` in a per-PID temp file (named, never written, in preview).
pub fn mcp_config_paths(cfg: &Config, preview: bool) -> Vec<String> {
    let procs = crate::sys::process_table();
    let rider: Vec<u32> = procs.iter().filter(|p| mcp::is_rider(&p.name)).map(|p| p.pid).collect();
    let ports = || mcp::rider_ports(&rider);
    let probe = mcp::Probe {
        rider_running: !rider.is_empty(),
        ports: &ports,
        is_mcp: mcp::is_mcp_port,
        temp: std::env::temp_dir(),
        pid: std::process::id(),
    };
    let (paths, notes) = mcp::config_paths(&cfg.extra_mcp_configs, cfg.rider_mcp, preview, &probe);
    print(&notes);
    paths
}

/// The catalog of this run, set once by `model_family_labels`; `None` inside = static labels.
static CATALOG: OnceLock<Option<models::Catalog>> = OnceLock::new();

/// `Initialize-ModelCatalog` + `Get-ModelFamilyLabels`: family -> display name read out of the
/// installed claude.exe, cached per binary. `None` keeps the static row labels.
pub fn model_family_labels(preview: bool) -> Option<BTreeMap<String, String>> {
    let home = home();
    let cat = CATALOG.get_or_init(|| {
        let loaded = models::load(&models::bin_path(&home), &home.join(r".claude\claude-auto-models.json"), preview);
        print(&catalog_note(loaded.as_ref()).into_iter().collect::<Vec<_>>());
        loaded.map(|c| c.0)
    });
    cat.as_ref().map(|c| models::family_labels(Some(c)).into_iter().collect())
}

/// The line `claude-auto.ps1` prints after a catalog rescan; a cache hit says nothing.
fn catalog_note(loaded: Option<&(models::Catalog, models::Source)>) -> Option<Note> {
    match loaded {
        Some((c, models::Source::Binary)) => Some(Note::new(
            format!("  model catalog re-read from claude.exe ({} models)", c.names.len()),
            Fg::DarkGray,
        )),
        Some((_, models::Source::Cache)) | None => None,
    }
}

/// `Get-FriendlyModelName`: a raw `--model` value to the label the model row shows for it.
pub fn friendly_model_name(raw: &str) -> String {
    models::friendly(raw, CATALOG.get().and_then(Option::as_ref))
}

fn read_settings(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).filter(|x| crate::config::ps_truthy(Some(x))).map(crate::config::ps_str)
}

/// `Get-DefaultModelLabel`: what 'default' on the model row means; a menu, never a validator.
pub fn default_model_label(settings: &Path) -> String {
    if !settings.exists() {
        return "default".into();
    }
    match read_settings(settings) {
        None => "default".into(),
        Some(v) => str_field(&v, "model").map_or_else(
            || "default (account default)".into(),
            |m| format!("default ({})", friendly_model_name(&m)),
        ),
    }
}

/// `Get-DefaultAdvisorLabel`: 'default (none)' = read and empty; 'default' = unreadable.
pub fn default_advisor_label(settings: &Path) -> String {
    if !settings.exists() {
        return "default".into();
    }
    match read_settings(settings) {
        None => "default".into(),
        Some(v) => str_field(&v, "advisorModel").map_or_else(|| "default (none)".into(), |a| format!("default ({a})")),
    }
}

/// `Get-DefaultLabels`: Row name -> the bare value 'default' resolves to for this profile.
pub fn default_labels(settings: &Path) -> DefaultLabels {
    let mut model = default_model_label(settings);
    if let Some(inner) = model.strip_prefix("default (").and_then(|m| m.strip_suffix(')')) {
        model = inner.to_owned();
    }
    if ["account default", "plan default", ""].contains(&model.as_str()) {
        model = "default".into();
    }
    let mut labels: DefaultLabels = [
        ("Model", model.as_str()),
        ("Advisor", "default"),
        ("Effort", "default"),
        ("Permission", "default"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
    .collect();
    if let Some(v) = settings.exists().then(|| read_settings(settings)).flatten() {
        if let Some(a) = str_field(&v, "advisorModel") {
            labels.insert("Advisor".into(), a);
        }
        if let Some(e) = str_field(&v, "effortLevel") {
            labels.insert("Effort".into(), e);
        }
        if let Some(p) = v.get("permissions").and_then(|p| str_field(p, "defaultMode")) {
            labels.insert("Permission".into(), p);
        }
    }
    labels
}

/// `Get-RateLimitSummary`: freshest of `<name>.json` / `<name>.widget.json` per account, age
/// text, availableModels three-state (widget only, 30 min max age).
pub fn rate_limit_summary(accounts: &[Account], dir: &Path, now_ms: i64) -> Limits {
    limits::summary(accounts, dir, now_ms, limits::AVAILABILITY_MAX_AGE_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_labels_read_settings_and_degrade() {
        let dir = std::env::temp_dir().join(format!("claude-auto-rs-env-{}", crate::log::new_run_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, r#"{"model":"opus","effortLevel":"high","permissions":{"defaultMode":"auto"}}"#).unwrap();
        let l = default_labels(&p);
        assert_eq!(
            l.get("Model").map(String::as_str),
            Some("Opus 5.5"),
            "through the friendly-name shortener"
        );
        assert_eq!(l.get("Effort").map(String::as_str), Some("high"));
        assert_eq!(l.get("Permission").map(String::as_str), Some("auto"));
        assert_eq!(l.get("Advisor").map(String::as_str), Some("default"));
        assert_eq!(default_advisor_label(&p), "default (none)");
        std::fs::write(&p, "{broken").unwrap();
        assert_eq!(default_model_label(&p), "default");
        assert_eq!(default_model_label(&dir.join("missing.json")), "default");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn canonical_profile_removes_config_dir_and_others_set_it() {
        let ctx = crate::config::PathCtx {
            home: r"C:\h".into(),
            cwd: r"C:\h".into(),
        };
        let cfg = crate::config::parse_config(include_str!("../tests/fixtures/config-four.json"), "x", &ctx);
        let mut child = ChildEnv::default();
        set_profile(&cfg, "personal", true, &mut child).unwrap();
        assert_eq!(child.effective("CLAUDE_CONFIG_DIR").as_deref(), Some(r"C:\h\.claude-acct2"));
        set_profile(&cfg, "work", true, &mut child).unwrap();
        assert_eq!(child.vars.last(), Some(&("CLAUDE_CONFIG_DIR".to_owned(), None)));
        assert!(set_profile(&cfg, "nope", true, &mut child).is_err());
    }

    fn four(home: &Path) -> Config {
        let ctx = crate::config::PathCtx {
            home: home.to_path_buf(),
            cwd: home.to_path_buf(),
        };
        crate::config::parse_config(include_str!("../tests/fixtures/config-four.json"), "x", &ctx)
    }

    #[test]
    fn preview_never_runs_the_mirror_and_each_secondary_gets_its_own_root() {
        let cfg = four(Path::new(r"C:\h"));
        let mut roots: Vec<String> = Vec::new();
        let mut rec = |r: &str| -> Result<String, String> {
            roots.push(r.to_owned());
            Ok(String::new())
        };
        let mut child = ChildEnv::default();
        let n = profile_notes(&cfg, "personal", true, &mut child, &mut rec).unwrap();
        assert_eq!(
            n.iter().filter(|x| x.text.contains("->")).count(),
            1,
            "preview still announces the account"
        );
        for k in ["personal", "shared", "low", "work"] {
            profile_notes(&cfg, k, false, &mut child, &mut rec).unwrap();
        }
        assert_eq!(
            roots,
            [r"C:\h\.claude-acct2", r"C:\h\.claude-shared", r"C:\h\.claude-low"],
            "once per secondary, never in preview"
        );
        let mut line = |_: &str| -> Result<String, String> { Ok("graph @ user".into()) };
        let n = profile_notes(&cfg, "low", false, &mut child, &mut line).unwrap();
        assert_eq!(n[1].text, "  MCP mirror: graph @ user");
        let mut bad = |_: &str| -> Result<String, String> { Err("claude-mirror-mcp: target file is not valid JSON".into()) };
        let n = profile_notes(&cfg, "low", false, &mut child, &mut bad).unwrap();
        assert_eq!(
            (n[1].text.as_str(), n[1].fg),
            (
                "  project MCP mirror failed: claude-mirror-mcp: target file is not valid JSON",
                Fg::DarkYellow
            )
        );
    }

    #[test]
    fn repair_in_preview_touches_nothing() {
        let home = std::env::temp_dir().join(format!("claude-auto-rs-preview-{}", crate::log::new_run_id()));
        let (work, personal) = (home.join(".claude"), home.join(".claude-acct2"));
        std::fs::create_dir_all(work.join("projects")).unwrap();
        std::fs::create_dir_all(&personal).unwrap();
        std::fs::write(work.join("settings.json"), r#"{"theme":"work"}"#).unwrap();
        std::fs::write(work.join("CLAUDE.md"), "# global").unwrap();
        std::fs::write(personal.join("settings.json"), r#"{"theme":"drifted"}"#).unwrap();
        let before = std::fs::metadata(personal.join("settings.json")).unwrap().modified().unwrap();
        repair_shared_profiles(&four(&home), true);
        assert_eq!(
            std::fs::read_to_string(personal.join("settings.json")).unwrap(),
            r#"{"theme":"drifted"}"#
        );
        assert_eq!(
            std::fs::metadata(personal.join("settings.json")).unwrap().modified().unwrap(),
            before
        );
        assert!(!personal.join("projects").exists(), "preview: no junction is created");
        assert!(!personal.join("CLAUDE.md").exists(), "symlink preview: nothing is created");
        assert!(!personal.join("settings.json.pre-relink").exists());
        assert!(!home.join(".claude-shared").exists(), "preview: no profile root is created");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn only_a_catalog_rescan_prints_its_line() {
        let mut cat = models::Catalog::default();
        cat.names.insert("claude-opus-5-5".into(), "Opus 5.5".into());
        cat.names.insert("claude-haiku-5-5".into(), "Haiku 5.5".into());
        assert_eq!(
            catalog_note(Some(&(cat.clone(), models::Source::Binary))),
            Some(Note::new("  model catalog re-read from claude.exe (2 models)", Fg::DarkGray))
        );
        assert_eq!(catalog_note(Some(&(cat, models::Source::Cache))), None, "a cache hit is silent");
        assert_eq!(catalog_note(None), None);
    }

    #[test]
    fn sorted_json_ignores_key_order_only() {
        let a: Value = serde_json::from_str(r#"{"b":[1,{"y":2,"x":1}],"a":"s"}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":"s","b":[1,{"x":1,"y":2}]}"#).unwrap();
        let c: Value = serde_json::from_str(r#"{"a":"s","b":[{"x":1,"y":2},1]}"#).unwrap();
        assert_eq!(sorted_json(&a), sorted_json(&b));
        assert_ne!(sorted_json(&a), sorted_json(&c));
        assert_eq!(sorted_json(&b), r#"{"a":"s","b":[1,{"x":1,"y":2}]}"#);
    }
}
