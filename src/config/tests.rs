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
    let text = include_str!("../../tests/fixtures/config-four.json");
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
