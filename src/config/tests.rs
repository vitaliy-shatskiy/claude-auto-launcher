use super::*;

fn ctx() -> PathCtx {
    PathCtx {
        home: PathBuf::from(r"C:\Users\someone"),
        cwd: PathBuf::from(r"C:\work"),
    }
}

#[test]
fn vsec_path_is_expanded_and_a_relative_one_is_refused() {
    let cfg = parse_config(r#"{"vsecPath": "~/tools/vsec.cmd"}"#, "p.json", &ctx());
    assert_eq!(cfg.vsec_path, r"C:\Users\someone\tools\vsec.cmd");
    assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
    let bad = parse_config(r#"{"vsecPath": "tools/vsec.cmd"}"#, "p.json", &ctx());
    assert_eq!(bad.vsec_path, "");
    assert_eq!(bad.warnings, ["vsecPath must be an absolute path; looking up vsec instead"]);
    assert_eq!(Config::defaults(&ctx(), "p.json").vsec_path, "", "unset by default");
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
    assert_eq!(cfg.launch_hooks, [[r"C:\abs.ps1"]]);
    assert_eq!(cfg.extra_mcp_configs, [] as [std::string::String; 0]);
    assert!(
        cfg.warnings
            .contains(&"launchHooks: 'rel.ps1' is not an absolute path; skipped".to_owned())
    );
}

#[test]
fn a_nested_hook_array_is_one_chain_and_a_string_its_own_entry() {
    let cfg = parse_config(
        r#"{"launchHooks":["C:/cbm.ps1",["C:/sync.ps1","C:/commit.ps1"],"C:/chrome.ps1"]}"#,
        "x",
        &ctx(),
    );
    assert_eq!(
        cfg.launch_hooks,
        [vec![r"C:\cbm.ps1"], vec![r"C:\sync.ps1", r"C:\commit.ps1"], vec![r"C:\chrome.ps1"]]
    );
    assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
    let single = parse_config(r#"{"launchHooks":"C:/one.ps1"}"#, "x", &ctx());
    assert_eq!(single.launch_hooks, [[r"C:\one.ps1"]], "a bare string stays one hook");
}

#[test]
fn a_deeper_level_or_a_non_string_in_a_chain_is_skipped_with_a_warning() {
    let cfg = parse_config(
        r#"{"launchHooks":[["C:/a.ps1",["C:/deep.ps1"],5,"rel.ps1","C:/b.ps1"],[],[{}],"C:/c.ps1"]}"#,
        "x",
        &ctx(),
    );
    assert_eq!(cfg.launch_hooks, [vec![r"C:\a.ps1", r"C:\b.ps1"], vec![r"C:\c.ps1"]]);
    assert_eq!(
        cfg.warnings,
        [
            "launchHooks: a hook chain holds only path strings, one level deep; [\"C:/deep.ps1\"] skipped",
            "launchHooks: a hook chain holds only path strings, one level deep; 5 skipped",
            "launchHooks: 'rel.ps1' is not an absolute path; skipped",
            "launchHooks: a hook chain holds only path strings, one level deep; {} skipped",
        ]
    );
}

#[test]
fn sharing_with_one_account_is_turned_off() {
    let cfg = parse_config(r#"{"sharing":true}"#, "x", &ctx());
    assert!(!cfg.sharing);
    assert_eq!(cfg.warnings, ["sharing needs at least two accounts; off"]);
}

#[test]
fn rows_key_orders_and_hides() {
    let cfg = parse_config(r#"{"rows":["Model","effort","bogus","model","wat","permission"]}"#, "x", &ctx());
    assert_eq!(
        cfg.rows,
        Some(vec![RowName::Account, RowName::Model, RowName::Effort, RowName::Permission]),
        "account first when unlisted, duplicates: the first wins, mode hidden"
    );
    assert_eq!(
        cfg.warnings,
        ["rows: unknown row name(s) 'bogus', 'wat' ignored (known: account, remote, model, effort, advisor, permission, mode)"]
    );
    let kept = parse_config(r#"{"rows":["mode","account"]}"#, "x", &ctx());
    assert_eq!(
        kept.rows,
        Some(vec![RowName::Mode, RowName::Account]),
        "a listed account keeps its place"
    );
    for bad in [
        r#"{"rows":"model"}"#,
        r#"{"rows":["nope"]}"#,
        r#"{"rows":[]}"#,
        r#"{"rows":["action"]}"#,
    ] {
        assert_eq!(parse_config(bad, "x", &ctx()).rows, None, "{bad}: every row");
    }
    assert_eq!(Config::defaults(&ctx(), "x").rows, None);
}

#[test]
fn model_order_key_lists_families_and_hides_the_rest() {
    let cfg = parse_config(r#"{"modelOrder":["Opus","haiku","opus","gpt"]}"#, "x", &ctx());
    assert_eq!(cfg.model_order, Some(vec!["opus".to_owned(), "haiku".to_owned()]));
    assert_eq!(
        cfg.warnings,
        ["modelOrder: unknown model famil(ies) 'gpt' ignored (known: haiku, sonnet, opus, fable)"]
    );
    assert_eq!(parse_config(r#"{"modelOrder":5}"#, "x", &ctx()).model_order, None);
    assert_eq!(Config::defaults(&ctx(), "x").model_order, None);
}

#[test]
fn project_hiding_keys_parse() {
    let cfg = parse_config(
        r#"{"hiddenProjects":["~/old","rel","D:/x/"],"hideHeadlessProjects":false}"#,
        "x",
        &ctx(),
    );
    assert_eq!(cfg.hidden_projects, [r"C:\Users\someone\old", r"D:\x\"]);
    assert!(!cfg.hide_headless_projects);
    assert_eq!(cfg.warnings, ["hiddenProjects: 'rel' is not an absolute path; skipped"]);
    assert!(Config::defaults(&ctx(), "x").hide_headless_projects, "on by default");
}

#[test]
fn project_screen_key_parses() {
    let on = parse_config(r#"{"projectScreen":"ondemand"}"#, "x", &ctx());
    assert_eq!(on.project_screen, ProjectScreen::OnDemand);
    assert!(on.warnings.is_empty(), "{:?}", on.warnings);
    assert_eq!(
        parse_config(r#"{"projectScreen":"always"}"#, "x", &ctx()).project_screen,
        ProjectScreen::Always
    );
    let bad = parse_config(r#"{"projectScreen":"never"}"#, "x", &ctx());
    assert_eq!(bad.project_screen, ProjectScreen::Always);
    assert_eq!(bad.warnings, ["projectScreen 'never' is not always/onDemand; using always"]);
    assert_eq!(Config::defaults(&ctx(), "x").project_screen, ProjectScreen::Always);
}

#[test]
fn mouse_key_parses() {
    assert!(!parse_config(r#"{"mouse":false}"#, "x", &ctx()).mouse);
    assert!(!parse_config(r#"{"mouse":"FALSE"}"#, "x", &ctx()).mouse);
    assert!(Config::defaults(&ctx(), "x").mouse, "on by default");
    let bad = parse_config(r#"{"mouse":"maybe"}"#, "x", &ctx());
    assert!(bad.mouse);
    assert_eq!(bad.warnings, ["mouse: 'maybe' is not a boolean; using True"]);
}

#[test]
fn save_keys_writes_its_keys_and_keeps_the_rest() {
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-save-keys-{}", crate::log::new_run_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("claude-auto.json");
    let p = path.to_str().unwrap();
    std::fs::write(&path, "\u{feff}{\"remote\": true, \"rows\": [\"model\"], \"zeta\": 1}").unwrap();
    save_keys(
        p,
        &[
            ("rows", serde_json::json!(["account", "mode"])),
            ("hiddenProjects", serde_json::json!([])),
        ],
    )
    .unwrap();
    let back: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        back.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["remote", "rows", "zeta", "hiddenProjects"],
        "other keys untouched, order kept"
    );
    assert_eq!(back["rows"], serde_json::json!(["account", "mode"]));
    std::fs::write(&path, "{ not json").unwrap();
    assert!(
        save_keys(p, &[("rows", serde_json::json!([]))]).is_err(),
        "an unparseable file is never overwritten"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    let fresh = dir.join("new.json");
    save_keys(fresh.to_str().unwrap(), &[("mouse", serde_json::json!(false))]).unwrap();
    assert!(!parse_config(&std::fs::read_to_string(&fresh).unwrap(), "x", &ctx()).mouse);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_keys_refuses_a_file_that_is_not_utf8() {
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-save-cp1251-{}", crate::log::new_run_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("claude-auto.json");
    // "Робота" saved as ANSI (cp1251): a lossy read would write U+FFFD over every letter.
    let bytes: &[u8] = b"{\"accounts\": [{\"key\": \"work\", \"label\": \"\xD0\xEE\xE1\xEE\xF2\xE0\"}]}";
    std::fs::write(&path, bytes).unwrap();
    assert!(save_keys(path.to_str().unwrap(), &[("mouse", serde_json::json!(false))]).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the file stays byte-exact");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn save_keys_writes_through_a_symlinked_config() {
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-save-link-{}", crate::log::new_run_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (target, link) = (dir.join("real.json"), dir.join("claude-auto.json"));
    std::fs::write(&target, r#"{"remote": true}"#).unwrap();
    if std::os::windows::fs::symlink_file(&target, &link).is_err() {
        // Creating a symlink needs Developer Mode or elevation; without it there is nothing to test.
        std::fs::remove_dir_all(&dir).unwrap();
        return;
    }
    save_keys(link.to_str().unwrap(), &[("mouse", serde_json::json!(false))]).unwrap();
    assert!(
        std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
        "the link survives"
    );
    let back: Value = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(
        back,
        serde_json::json!({"remote": true, "mouse": false}),
        "the target took the write"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn l_is_reserved_for_the_rows_editor() {
    let (a, w) = parse_actions(&serde_json::json!([{"key":"l","script":"C:/x"}]), &ctx());
    assert_eq!(a, []);
    assert!(w[0].contains("outside u,r,d,m,p,l"), "{w:?}");
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
