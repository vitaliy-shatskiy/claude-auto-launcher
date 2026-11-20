//! Plugins through the built binary: a `.cmd` plugin listed in config describes a row, its value
//! reaches `preLaunch`, claude starts with the env and args it returns, `postLaunch` hears the exit
//! code, and a `cancel` stops the launch.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{launcher, scratch, text};

const DESCRIBE: &str = r#"{"protocol":1,"rows":[{"name":"ticket","label":"ticket","values":["none","ABC-1"]}],"hooks":["preLaunch","postLaunch"]}"#;

/// `tick.cmd`: answers `describe`, saves what `preLaunch` and `postLaunch` were sent, and answers
/// `preLaunch` with `pre`.
fn plugin(dir: &Path, pre: &str) -> PathBuf {
    let p = dir.join("tick.cmd");
    let body = format!(
        "@echo off\r\ngoto %~1\r\n:describe\r\necho {DESCRIBE}\r\nexit /b 0\r\n:preLaunch\r\nfindstr \"^\" > \"%~dp0pre-in.json\"\r\necho {pre}\r\nexit /b 0\r\n:postLaunch\r\nfindstr \"^\" > \"%~dp0post-in.json\"\r\necho {{}}\r\nexit /b 0\r\n"
    );
    std::fs::write(&p, body).unwrap();
    p
}

fn configured(dir: &Path, pre: &str) -> Command {
    let tick = plugin(dir, pre);
    let config = serde_json::json!({
        "accounts": [{ "key": "main", "root": "~/.claude", "label": "main", "tint": "Green" }],
        "plugins": [{ "name": "tick", "path": tick.to_string_lossy() }]
    });
    let file = dir.join("home").join("config.json");
    std::fs::write(&file, config.to_string()).unwrap();
    std::fs::write(
        dir.join("bin").join("claude.cmd"),
        format!(
            "@echo ARGV %*\r\n@echo SAW [%PLUGIN_SAW%]\r\n@echo. > \"{}\"\r\n@exit /b 7\r\n",
            dir.join("claude-started").display()
        ),
    )
    .unwrap();
    again(dir)
}

/// The launcher over the config `configured` wrote, the plugin file untouched.
fn again(dir: &Path) -> Command {
    let mut c = launcher(dir);
    c.env("CLAUDE_AUTO_CONFIG", dir.join("home").join("config.json"));
    c
}

fn run_line(project: &Path) -> String {
    serde_json::json!({
        "account": "main", "project": project.to_string_lossy(), "action": "new",
        "model": "haiku", "effort": "high", "advisor": "off", "permission": "auto", "mode": "safe",
        "pluginRows": { "tick.ticket": "ABC-1" }
    })
    .to_string()
}

#[test]
fn a_plugin_row_reaches_pre_launch_and_claude_starts_with_its_env_and_args() {
    let d = scratch("plugin-go");
    let mut c = configured(&d, r#"{"env":{"PLUGIN_SAW":"yes"},"args":["--plugin-arg"]}"#);
    let o = c.args(["--launcher-run", &run_line(&d)]).output().unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(7), "{out}");
    assert!(out.contains("SAW [yes]"), "the env reached claude: {out}");
    assert!(out.contains("--plugin-arg"), "the args reached claude: {out}");
    let pre = std::fs::read_to_string(d.join("pre-in.json")).unwrap();
    assert!(pre.contains(r#""rows":{"ticket":"ABC-1"}"#), "{pre}");
    let post = std::fs::read_to_string(d.join("post-in.json")).unwrap();
    assert!(post.contains(r#""exitCode":7"#), "{post}");

    // The describe answer is cached now: the screenless listing shows the row without a call.
    let options = again(&d).arg("--launcher-options").output().unwrap();
    let v: serde_json::Value = serde_json::from_str(text(&options).trim_end()).unwrap();
    assert_eq!(v["pluginRows"][0]["name"], "tick.ticket", "{v}");
    assert_eq!(v["pluginRows"][0]["values"], serde_json::json!(["none", "ABC-1"]));
    let preview = again(&d)
        .env("CLAUDE_AUTO_PREVIEW", "1")
        .env("CLAUDE_AUTO_PREVIEW_KEYS", "Escape")
        .output()
        .unwrap();
    assert!(text(&preview).contains("ticket"), "the row is on the launch screen: {}", text(&preview));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_cancel_from_pre_launch_stops_the_launch_with_its_reason() {
    let d = scratch("plugin-cancel");
    let mut c = configured(&d, r#"{"cancel":"no ticket picked"}"#);
    let o = c.args(["--launcher-run", &run_line(&d)]).output().unwrap();
    let out = text(&o);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("launch cancelled by plugin tick: no ticket picked"), "{out}");
    assert!(!d.join("claude-started").exists(), "claude never started");
    std::fs::remove_dir_all(&d).unwrap();
}
