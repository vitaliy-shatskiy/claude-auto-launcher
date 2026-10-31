//! Environment around a launch (`Env.ps1`, `sharing/`). PHASE B, worker W3 (see PORTING.md):
//! profiles and the MCP mirror, secrets, sharing repair (hardlinks, symlinks, junctions, ACL),
//! launch hooks, MCP discovery incl. Rider, the model catalog, rate limits. The signatures are
//! final; the bodies marked STUB keep the launch path working until W3 replaces them.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::config::{Account, Config};
use crate::out::{self, Fg};
use crate::types::{ChildEnv, DefaultLabels, Limits};

/// `Set-ClaudeProfile`: the canonical account is the ABSENCE of `CLAUDE_CONFIG_DIR`. Prints the
/// account line in its tint; with sharing on and outside preview, mirrors project MCP servers.
pub fn set_profile(cfg: &Config, account: &str, preview: bool, child: &mut ChildEnv) -> Result<(), String> {
    let Some(a) = cfg.accounts.iter().find(|a| a.key == account) else {
        let keys: Vec<&str> = cfg.accounts.iter().map(|a| a.key.as_str()).collect();
        return Err(format!("unknown account '{account}' (roster: {})", keys.join(", ")));
    };
    if a.canonical {
        child.remove("CLAUDE_CONFIG_DIR");
        out::line(&format!("-> {}", a.label), Fg::from_tint(&a.tint));
        return Ok(());
    }
    child.set("CLAUDE_CONFIG_DIR", &a.root);
    out::line(&format!("-> {}", a.label), Fg::from_tint(&a.tint));
    if !cfg.sharing || preview {
        return Ok(());
    }
    match mirror_project_mcp(&a.root) {
        Ok(msg) if !msg.is_empty() => out::line(&format!("  MCP mirror: {msg}"), Fg::DarkGray),
        Ok(_) => {}
        Err(msg) => out::line(&format!("  project MCP mirror failed: {msg}"), Fg::DarkYellow),
    }
    Ok(())
}

/// STUB (W3): `sharing/claude-mirror-mcp.mjs` run against `root`; Ok(one-line summary).
pub fn mirror_project_mcp(_root: &str) -> Result<String, String> {
    Ok(String::new())
}

/// STUB (W3): `Import-ProjectSecrets` - one file per variable, tiers shared -> org -> project,
/// reserved shell names refused. Adds to the CHILD environment; returns how many were imported.
pub fn import_project_secrets(_cwd: &Path, _secrets_root: &str, _child: &mut ChildEnv) -> usize {
    0
}

/// `Set-ClaudeProjectDirectory`: the session starts in the chosen project. A path that is not an
/// existing directory is refused and the launch directory stays.
pub fn set_project_directory(project: &str) -> bool {
    if project.is_empty() || !Path::new(project).is_dir() {
        return false;
    }
    std::env::set_current_dir(project).is_ok()
}

/// STUB (W3): `Repair-SharedProfiles` - hardlinks, file symlinks, junctions and their ACL guard.
pub fn repair_shared_profiles(_cfg: &Config, _preview: bool) {}

/// STUB (W3): `Invoke-LaunchHooks` - each configured hook, best-effort, one line each.
pub fn run_launch_hooks(_hooks: &[String]) {}

/// STUB (W3): `Get-McpConfigPaths` - extra configs, the Rider MCP endpoint (riderMcp), per-PID
/// temp files. The stub passes the configured extra files that exist.
pub fn mcp_config_paths(cfg: &Config, _preview: bool) -> Vec<String> {
    cfg.extra_mcp_configs.iter().filter(|p| Path::new(p).is_file()).cloned().collect()
}

/// STUB (W3): `Initialize-ModelCatalog` + `Get-ModelFamilyLabels` - family -> display name read
/// out of the installed claude.exe, cached per binary. `None` keeps the static row labels.
pub fn model_family_labels(_preview: bool) -> Option<BTreeMap<String, String>> {
    None
}

/// STUB (W3): `Get-FriendlyModelName` - a raw `--model` id to its short label.
pub fn friendly_model_name(raw: &str) -> String {
    raw.to_owned()
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

/// STUB (W3): `Get-RateLimitSummary` - freshest of `<name>.json` / `<name>.widget.json` per
/// account, age text, availableModels three-state (widget only, 30 min max age).
pub fn rate_limit_summary(_accounts: &[Account], _dir: &Path, _now_ms: i64) -> Limits {
    Limits::new()
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
        assert_eq!(l.get("Model").map(String::as_str), Some("opus"));
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
}
