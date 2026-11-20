//! Plugins through the built binary: a `.cmd` plugin listed in config describes a row, its value
//! reaches `preLaunch`, claude starts with the env and args it returns, `postLaunch` hears the exit
//! code, and a `cancel` stops the launch. Then the runner itself on real `.cmd` plugins: a crash,
//! bad JSON, a hang killed with its tree, a file not listed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use claude_auto::config::PluginSpec;
use claude_auto::plugins::run::{self, Failure, Process, Runner as _};
use claude_auto::plugins::{self, describe};
use serde_json::json;

use super::{launcher, scratch, text};

const DESCRIBE: &str =
    r#"{"protocol":1,"rows":[{"name":"ticket","label":"ticket","values":["none","ABC-1"]}],"hooks":["preLaunch","postLaunch"]}"#;

/// `tick.cmd`: answers `describe` (and leaves `described` beside itself), saves what `preLaunch`
/// and `postLaunch` were sent, and answers `preLaunch` with `pre`.
fn plugin(dir: &Path, pre: &str) -> PathBuf {
    let p = dir.join("tick.cmd");
    let body = format!(
        "@echo off\r\ngoto %~1\r\n:describe\r\necho.> \"%~dp0described\"\r\necho {DESCRIBE}\r\nexit /b 0\r\n:preLaunch\r\nfindstr \"^\" > \"%~dp0pre-in.json\"\r\necho {pre}\r\nexit /b 0\r\n:postLaunch\r\nfindstr \"^\" > \"%~dp0post-in.json\"\r\necho {{}}\r\nexit /b 0\r\n"
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
    assert!(
        text(&preview).contains("ticket"),
        "the row is on the launch screen: {}",
        text(&preview)
    );
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

#[test]
fn a_refused_launcher_run_starts_no_plugin_and_writes_no_plugin_cache() {
    let d = scratch("plugin-refused");
    let mut c = configured(&d, "{}");
    let mut line: serde_json::Value = serde_json::from_str(&run_line(&d)).unwrap();
    line["account"] = "nobody".into();
    let o = c.args(["--launcher-run", &line.to_string()]).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(!d.join("described").exists(), "describe never ran");
    assert!(!d.join("home").join("claude-auto-plugins.json").exists(), "no plugin cache written");
    assert!(!d.join("claude-started").exists(), "claude never started");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_bad_plugin_row_value_is_still_refused_once_the_plugins_are_described() {
    let d = scratch("plugin-bad-row");
    let mut c = configured(&d, "{}");
    let mut line: serde_json::Value = serde_json::from_str(&run_line(&d)).unwrap();
    line["pluginRows"]["tick.ticket"] = "XYZ-9".into();
    let o = c.args(["--launcher-run", &line.to_string()]).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(String::from_utf8_lossy(&o.stderr).contains("pluginRows"), "says which field");
    assert!(d.join("described").exists(), "the row's plugin was described first");
    assert!(!d.join("claude-started").exists(), "claude never started");
    std::fs::remove_dir_all(&d).unwrap();
}

// --- the runner on real processes: crash, bad JSON, a hang, an unlisted file ---

/// `<name>.cmd` in `d` running `body`, listed with a 5 s timeout.
fn cmd(d: &Path, name: &str, body: &str) -> PluginSpec {
    let p = d.join(format!("{name}.cmd"));
    std::fs::write(&p, format!("@echo off\r\n{body}\r\n")).unwrap();
    PluginSpec {
        name: name.into(),
        path: p.to_string_lossy().into_owned(),
        args: Vec::new(),
        timeout_ms: Some(5_000),
    }
}

/// Names of the processes still alive among `pid` and its children. conhost.exe is the console
/// host Windows starts beside a console child, outside the job; it exits on its own once its
/// client is gone (seen in the full suite).
fn tree_left(pid: u32) -> String {
    let query = format!(
        "Get-CimInstance Win32_Process -Filter \"(ProcessId={pid} or ParentProcessId={pid}) and Name<>'conhost.exe'\" | ForEach-Object Name"
    );
    let o = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &query])
        .output()
        .unwrap();
    assert!(o.status.success(), "the process query ran");
    String::from_utf8_lossy(&o.stdout).trim().to_owned()
}

#[test]
fn a_crash_bad_json_and_a_good_answer_through_real_processes() {
    let d = scratch("plugin-crash");
    let crash = cmd(&d, "crash", "exit /b 3");
    let bad = cmd(&d, "bad", "echo {not json");
    let good = cmd(&d, "good", r#"echo {"protocol":1}"#);
    assert!(matches!(Process.call(&crash, &describe::VERB, &json!({})), Err(Failure::Exit(3))));
    assert!(matches!(Process.call(&bad, &describe::VERB, &json!({})), Err(Failure::BadJson(_))));
    let (p, w) = plugins::load(&[crash, bad, good], None, &Process, true);
    assert_eq!(
        p.list.iter().map(|l| l.spec.name.as_str()).collect::<Vec<_>>(),
        ["good"],
        "the rest goes on"
    );
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w.first().is_some_and(|l| l.starts_with("plugin crash describe: exit 3")), "{w:?}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_hang_past_its_timeout_is_killed_with_its_tree() {
    let d = scratch("plugin-hang");
    let mut hang = cmd(&d, "hang", "ping -n 30 127.0.0.1");
    hang.timeout_ms = Some(300);
    let Err(Failure::Timeout { ms, pid }) = Process.call(&hang, &describe::VERB, &json!({})) else {
        panic!("a timeout");
    };
    assert_eq!(ms, 300);
    assert_eq!(tree_left(pid), "", "the plugin and its ping are gone");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_file_not_listed_never_runs() {
    let d = scratch("plugin-unlisted");
    let marker = d.join("ran.txt");
    cmd(&d, "evil", &format!("echo x > \"{}\"\r\necho {{\"protocol\":1}}", marker.display()));
    let listed = cmd(&d, "listed", r#"echo {"protocol":1}"#);
    let (p, _) = plugins::load(&[listed], None, &Process, true);
    assert_eq!(p.list.len(), 1);
    assert!(!marker.exists(), "a file beside a listed one is never started");
    let missing = PluginSpec {
        name: "cmd".into(),
        path: d.join("cmd.exe").to_string_lossy().into_owned(),
        args: Vec::new(),
        timeout_ms: None,
    };
    assert!(
        matches!(Process.call(&missing, &describe::VERB, &json!({})), Err(Failure::Missing)),
        "a missing listed file is never looked up on PATH"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_timed_out_call_returns_once_its_tree_is_killed_not_when_the_hang_ends() {
    let d = scratch("plugin-hang-return");
    let mut hang = cmd(&d, "hang", "ping -n 30 127.0.0.1");
    hang.timeout_ms = Some(300);
    let started = Instant::now();
    let answer = run::call(&hang, &describe::VERB, &json!({}), Duration::from_secs(120));
    let took = started.elapsed();
    assert!(matches!(answer, Err(Failure::Timeout { ms: 300, .. })), "{answer:?}");
    // A wall-clock bound on purpose, kept far from both outcomes: without the kill the pipe stays
    // open until ping ends (29 s at least) inside the 120 s grace; with it the call is back right
    // after the 300 ms timeout. 15 s leaves room for a loaded machine and still fails the first.
    assert!(took < Duration::from_secs(15), "returned after {took:?}: the tree was not killed");
    std::fs::remove_dir_all(&d).unwrap();
}
