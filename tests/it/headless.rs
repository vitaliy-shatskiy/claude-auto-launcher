//! `--launcher-options` and `--launcher-run` through the built binary, over a fake home. A run that
//! must leave no trace is checked against a snapshot of the whole scratch tree (temp included) and
//! a fake `claude` that leaves a file behind when anything starts it.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{launcher, scratch, text};

const PREFS: &str =
    r#"{"Version":2,"Account":"alt","Profiles":{"main":{"Effort":"medium","Remote":"off"},"alt":{"Remote":"off","Model":"fable"}}}"#;

fn api_launcher(dir: &Path) -> Command {
    std::fs::write(
        dir.join("home").join("config.json"),
        r#"{"accounts":[{"key":"main","root":"~/.claude","label":"main acc","tint":"Green"},{"key":"alt","root":"~/.claude-alt","label":"alt acc","tint":"Magenta"}],"remote":true}"#,
    )
    .unwrap();
    std::fs::write(dir.join("prefs.json"), PREFS).unwrap();
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let mut c = launcher(dir);
    c.env("CLAUDE_AUTO_CONFIG", dir.join("home").join("config.json"))
        .env("TEMP", &tmp)
        .env("TMP", &tmp);
    c
}

/// A fake `claude` that says where it was started and leaves `claude-started` behind.
fn marking_claude(dir: &Path) {
    let marker = dir.join("claude-started");
    std::fs::write(
        dir.join("bin").join("claude.cmd"),
        format!(
            "@echo ARGV %*\r\n@echo CWD [%CD%]\r\n@echo CONFIG [%CLAUDE_CONFIG_DIR%]\r\n@echo. > \"{}\"\r\n@exit /b 7\r\n",
            marker.display()
        ),
    )
    .unwrap();
}

/// Every file under `dir` with its size and write time, and every directory by name: a
/// directory's own time in a listing is updated lazily on NTFS, so it proves nothing either way.
fn tree(dir: &Path) -> Vec<(PathBuf, u64, Option<std::time::SystemTime>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let m = std::fs::metadata(e.path()).unwrap();
            if m.is_dir() {
                stack.push(e.path());
                out.push((e.path(), 0, None));
            } else {
                out.push((e.path(), m.len(), m.modified().ok()));
            }
        }
    }
    out.sort();
    out
}

fn run_json(project: &Path, extra: &[(&str, serde_json::Value)]) -> String {
    let mut v = serde_json::json!({
        "account": "main",
        "project": project.to_string_lossy(),
        "action": "new",
        "model": "haiku",
        "effort": "high",
        "advisor": "off",
        "permission": "auto",
        "mode": "safe",
    });
    for (k, x) in extra {
        if x.is_null() {
            v.as_object_mut().unwrap().remove(*k);
        } else {
            v[*k] = x.clone();
        }
    }
    v.to_string()
}

#[test]
fn launcher_options_prints_one_line_of_json_and_leaves_no_trace() {
    let d = scratch("options");
    marking_claude(&d);
    let mut cmd = api_launcher(&d);
    let rate = d.join("home").join(".claude").join("rate-limits");
    std::fs::create_dir_all(&rate).unwrap();
    std::fs::write(rate.join("main.json"), r#"{"fiveHour":16,"sevenDay":3,"atMs":1800000000000}"#).unwrap();
    let versions = d.join("home").join(".local").join("share").join("claude").join("versions");
    std::fs::create_dir_all(&versions).unwrap();
    std::fs::write(versions.join("2.1.99"), "x").unwrap();
    let before = tree(&d);

    let o = cmd.arg("--launcher-options").output().unwrap();

    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let out = text(&o);
    assert_eq!(out.trim_end().lines().count(), 1, "one line, nothing else on stdout: {out}");
    let v: serde_json::Value = serde_json::from_str(out.trim_end()).unwrap();
    assert_eq!(v["launcher"], "claude-auto");
    assert_eq!(v["version"], "2.1.99");
    assert_eq!(v["accounts"][0]["key"], "main");
    assert_eq!(v["accounts"][0]["limitPct"], 16);
    assert_eq!(v["accounts"][0]["prefs"]["effort"], "medium");
    assert_eq!(v["accounts"][1]["limitPct"], serde_json::Value::Null);
    assert!(
        v["rows"].as_array().unwrap().iter().all(|r| r["name"] != "remote"),
        "the remote row is the PC's: {v}"
    );
    assert!(!d.join("claude-started").exists(), "nothing started claude");
    assert_eq!(tree(&d), before, "not one file written, touched or created");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn launcher_options_keeps_config_warnings_off_the_json() {
    let d = scratch("options-warn");
    let mut c = api_launcher(&d);
    std::fs::write(d.join("home").join("config.json"), r#"{"nonsense":1}"#).unwrap();
    let o = c.arg("--launcher-options").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&o.stderr).is_empty(), "the warning is still said");
    let _: serde_json::Value = serde_json::from_str(text(&o).trim_end()).unwrap();
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn launcher_options_refuses_extra_arguments_rather_than_passing_them_to_claude() {
    let d = scratch("options-extra");
    marking_claude(&d);
    let o = api_launcher(&d).args(["--launcher-options", "--x"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(!d.join("claude-started").exists());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn launcher_run_refuses_a_bad_request_before_any_side_effect() {
    let d = scratch("run-bad");
    marking_claude(&d);
    let missing = d.join("no-such-dir").to_string_lossy().into_owned();
    let cases = [
        ("unknown account", run_json(&d, &[("account", "nobody".into())])),
        ("value outside the row", run_json(&d, &[("model", "gpt".into())])),
        ("a dropped row", run_json(&d, &[("effort", serde_json::Value::Null)])),
        ("the project action", run_json(&d, &[("action", "project".into())])),
        ("missing directory", run_json(&d, &[("project", missing.into())])),
        ("resume without an id", run_json(&d, &[("action", "resume".into())])),
        ("unknown key", run_json(&d, &[("whatever", 1.into())])),
        ("not JSON", "--model haiku".to_owned()),
    ];
    api_launcher(&d);
    let before = tree(&d);
    for (name, line) in cases {
        let mut c = launcher(&d);
        c.env("CLAUDE_AUTO_CONFIG", d.join("home").join("config.json"))
            .env("TEMP", d.join("tmp"))
            .env("TMP", d.join("tmp"));
        let o = c.args(["--launcher-run", &line]).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{name}: {}", text(&o));
        assert!(!String::from_utf8_lossy(&o.stderr).is_empty(), "{name}: says why");
        assert!(!d.join("claude-started").exists(), "{name}: claude did not start");
        assert_eq!(tree(&d), before, "{name}: no log, no prefs, no file of any kind");
    }
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn launcher_run_starts_claude_in_the_project_and_saves_no_choice() {
    let d = scratch("run");
    marking_claude(&d);
    let project = d.join("the project");
    std::fs::create_dir_all(&project).unwrap();
    let mut c = api_launcher(&d);
    let line = run_json(
        &project,
        &[
            ("account", "alt".into()),
            ("action", "resume".into()),
            ("resumeId", "abc".into()),
            ("fork", true.into()),
        ],
    );

    let o = c.args(["--launcher-run", &line]).output().unwrap();
    let out = text(&o);

    assert_eq!(o.status.code(), Some(7), "claude's own code comes back: {out}");
    assert!(
        out.contains("--model haiku --effort high --permission-mode auto --safe-mode --resume abc --fork-session"),
        "the screen's arguments: {out}"
    );
    assert!(out.contains(&format!("CWD [{}]", project.display())), "{out}");
    assert!(out.contains(r".claude-alt]"), "the account's profile: {out}");
    assert_eq!(
        std::fs::read_to_string(d.join("prefs.json")).unwrap(),
        PREFS,
        "the choices of a screenless launch are not remembered"
    );
    assert!(
        d.join("home").join(".claude").join("launcher-logs").exists(),
        "the launch logs as one from the screen does"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn launcher_run_in_preview_goes_the_same_way_and_launches_and_writes_nothing() {
    let d = scratch("run-preview");
    marking_claude(&d);
    let mut c = api_launcher(&d);
    let before = tree(&d);
    let o = c
        .env("CLAUDE_AUTO_PREVIEW", "1")
        .args(["--launcher-run", &run_json(&d, &[("action", "worktree".into())])])
        .output()
        .unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("--- preview, nothing launched ---"), "no screen first: {out}");
    assert!(out.contains("account            : main"), "{out}");
    assert!(out.contains(&format!("project            : {}", d.display())), "{out}");
    assert!(out.contains("remote             : False"), "the remembered off is obeyed: {out}");
    assert!(
        out.contains("--model haiku --effort high --permission-mode auto --safe-mode -w"),
        "{out}"
    );
    assert!(out.contains("disabled (CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1)"), "{out}");
    assert!(!d.join("claude-started").exists());
    assert_eq!(tree(&d), before, "preview writes nothing");
    std::fs::remove_dir_all(&d).unwrap();
}
