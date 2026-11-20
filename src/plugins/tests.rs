//! The protocol through a fake runner (rows, preLaunch, actions, sources, the run gate) and
//! through real `.cmd` plugins (crash, hang, bad JSON, an unlisted file).

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;

use super::run::{Failure, Process};
use super::*;
use crate::input::KeyPress;
use crate::launch::tests::four;
use crate::launch::{OptionText, step_launch_value};
use crate::types::DefaultLabels;

/// Answers per verb; records every call as (plugin, verb, input).
#[derive(Default)]
struct Fake {
    answers: Vec<(&'static str, Value)>,
    calls: Mutex<Vec<(String, String, Value)>>,
}

impl Fake {
    fn with(answers: &[(&'static str, Value)]) -> Self {
        Self {
            answers: answers.to_vec(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self, verb: &str) -> Vec<Value> {
        self.calls.lock().unwrap().iter().filter(|c| c.1 == verb).map(|c| c.2.clone()).collect()
    }
}

impl Runner for Fake {
    fn call(&self, spec: &PluginSpec, verb: &Verb, input: &Value) -> Result<Value, Failure> {
        self.calls.lock().unwrap().push((spec.name.clone(), verb.name.into(), input.clone()));
        self.answers
            .iter()
            .find(|(v, _)| *v == verb.name)
            .map(|(_, a)| a.clone())
            .ok_or(Failure::Exit(9))
    }
}

fn spec(name: &str, path: &str) -> PluginSpec {
    PluginSpec {
        name: name.into(),
        path: path.into(),
        args: Vec::new(),
        timeout_ms: None,
    }
}

fn ticket() -> Value {
    json!({"protocol":1,"rows":[{"name":"ticket","label":"ticket","values":["none","ABC-1"]}],
        "actions":[{"key":"j","label":"jira","screen":"launch"}],
        "hooks":["preLaunch","postLaunch"],"sources":["projects","sessions"]})
}

fn loaded(fake: &Fake) -> Plugins {
    let (p, w) = load(&[spec("tick", r"C:\p\tick.exe")], None, fake, true);
    assert_eq!(w, Vec::<String>::new());
    p
}

#[test]
fn a_described_row_reaches_the_screen_and_its_value_reaches_pre_launch() {
    let fake = Fake::with(&[("describe", ticket()), ("preLaunch", json!({}))]);
    let plugins = loaded(&fake);
    let mut roster = four();
    plugins.add_rows(&mut roster);
    let row = roster.rows.iter().position(|r| r.label == "ticket").expect("the row is on the screen");
    let mut state = roster.new_state();
    assert_eq!(state.get(RowName::Plugin(0)), "none", "starts on its default");
    state.row = row;
    let labels = DefaultLabels::new();
    step_launch_value(&roster, &mut state, 1, &OptionText::plain(None, &labels));
    let mut w = Vec::new();
    let out = pre_launch::run(&plugins, &|i| plugins.choice(i, &state, None), &fake, &mut w);
    assert_eq!(out, pre_launch::Outcome::Go(pre_launch::Extra::default()));
    assert_eq!(fake.calls("preLaunch"), [json!({"choice": plugins.choice(0, &state, None)})]);
    assert_eq!(fake.calls("preLaunch")[0]["choice"]["rows"], json!({"ticket": "ABC-1"}));
}

#[test]
fn pre_launch_env_and_args_come_back_and_cancel_stops() {
    let go = Fake::with(&[("describe", ticket()), ("preLaunch", json!({"env":{"TICKET":"ABC-1"},"args":["--x"]}))]);
    let plugins = loaded(&go);
    let state = four().new_state();
    let mut w = Vec::new();
    let out = pre_launch::run(&plugins, &|i| plugins.choice(i, &state, None), &go, &mut w);
    let want = pre_launch::Extra {
        env: vec![("TICKET".into(), "ABC-1".into())],
        args: vec!["--x".into()],
    };
    assert_eq!(out, pre_launch::Outcome::Go(want));
    let stop = Fake::with(&[("describe", ticket()), ("preLaunch", json!({"cancel":"no ticket picked"}))]);
    let out = pre_launch::run(&plugins, &|i| plugins.choice(i, &state, None), &stop, &mut w);
    assert_eq!(
        out,
        pre_launch::Outcome::Cancel {
            plugin: "tick".into(),
            reason: "no ticket picked".into()
        }
    );
    assert_eq!(w, Vec::<String>::new());
}

#[test]
fn pre_launch_output_with_a_nul_or_a_bad_name_adds_nothing_and_warns() {
    let state = four().new_state();
    for bad in [
        json!({"env":{"A":"x\u{0}y"}}),
        json!({"env":{"A=B":"x"}}),
        json!({"args":["a\u{0}"]}),
        json!({"env":{"A":1}}),
        json!({"cancel":5}),
    ] {
        let fake = Fake::with(&[("describe", ticket()), ("preLaunch", bad.clone())]);
        let plugins = loaded(&fake);
        let mut w = Vec::new();
        let out = pre_launch::run(&plugins, &|i| plugins.choice(i, &state, None), &fake, &mut w);
        assert_eq!(out, pre_launch::Outcome::Go(pre_launch::Extra::default()), "{bad}");
        assert_eq!(w.len(), 1, "{bad}: {w:?}");
    }
}

#[test]
fn a_failing_pre_launch_warns_and_the_launch_goes_on() {
    let fake = Fake::with(&[("describe", ticket())]);
    let plugins = loaded(&fake);
    let state = four().new_state();
    let mut w = Vec::new();
    let out = pre_launch::run(&plugins, &|i| plugins.choice(i, &state, None), &fake, &mut w);
    assert_eq!(out, pre_launch::Outcome::Go(pre_launch::Extra::default()));
    assert_eq!(w, ["plugin tick preLaunch: exit 9; skipped"]);
}

#[test]
fn a_declared_key_runs_action_and_its_message_comes_back() {
    let fake = Fake::with(&[("describe", ticket()), ("action", json!({"message":"opened ABC-1"}))]);
    let plugins = loaded(&fake);
    let j = KeyPress {
        ch: 'j',
        vk: u16::from(b'J'),
        ..KeyPress::default()
    };
    let (p, key) = action::find(&plugins, "launch", &j).expect("j is the plugin's key");
    let line = action::run(&plugins, p, key, &json!({"screen":"launch"}), &fake);
    assert_eq!(line.as_deref(), Some("tick: opened ABC-1"));
    assert_eq!(fake.calls("action"), [json!({"key":"j","context":{"screen":"launch"}})]);
    let ctrl_j = KeyPress { ctrl: true, ..j };
    assert_eq!(action::find(&plugins, "launch", &ctrl_j), None);
}

#[test]
fn a_key_a_second_plugin_also_declares_is_refused_with_a_warning() {
    let fake = Fake::with(&[("describe", ticket())]);
    let (p, w) = load(&[spec("one", r"C:\p\1.exe"), spec("two", r"C:\p\2.exe")], None, &fake, true);
    assert_eq!(p.list.iter().map(|l| l.desc.actions.len()).collect::<Vec<_>>(), [1, 0]);
    assert_eq!(w, ["plugin two: action 'j' is taken by plugin one; skipped"]);
}

#[test]
fn source_entries_merge_into_the_lists_marked_by_plugin() {
    let fake = Fake::with(&[
        ("describe", ticket()),
        ("projects", json!([{"path":r"C:\jira\ABC","name":"ABC","lastUsed":5},{"path":"relative"}])),
        ("sessions", json!([{"id":"s-1","title":"from jira","updated":7},{"id":"-bad"}])),
    ]);
    let plugins = loaded(&fake);
    let mut w = Vec::new();
    let extra = projects::collect(&plugins, &fake, &mut w);
    let known = crate::projects::Project {
        name: "known".into(),
        path: r"C:\known".into(),
        slug: "C--known".into(),
        slugs: vec!["C--known".into()],
        worktree: None,
        last_activity_ms: Some(9),
        hidden: false,
    };
    let merged = projects::merge(std::slice::from_ref(&known), &extra);
    assert_eq!(merged.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["known", "ABC [tick]"]);
    assert_eq!(w.len(), 1, "the relative path is named: {w:?}");
    let s = sessions::collect(&plugins, r"C:\known", &fake, &mut w);
    assert_eq!(
        s.iter().map(|s| (s.session_id.as_str(), s.account.as_str(), s.title.as_str())).collect::<Vec<_>>(),
        [("s-1", "tick", "from jira")]
    );
    assert_eq!(fake.calls("sessions"), [json!({"project": r"C:\known"})]);
}

#[test]
fn a_project_the_registry_knows_keeps_its_own_row() {
    let known = crate::projects::Project {
        name: "known".into(),
        path: r"C:\known".into(),
        slug: String::new(),
        slugs: Vec::new(),
        worktree: None,
        last_activity_ms: None,
        hidden: false,
    };
    let e = projects::Entry {
        plugin: "tick".into(),
        path: r"c:\KNOWN\".into(),
        name: "dup".into(),
        last_used_ms: None,
    };
    assert_eq!(projects::merge(std::slice::from_ref(&known), &[e]), [known]);
}

#[test]
fn a_plugin_session_joins_the_picker_and_resumes_without_a_transfer() {
    let d = dir("picker");
    let req = crate::sessions::PickerRequest {
        sessions_root: d.clone(),
        others: &[],
        project_slugs: &[],
        project_name: "",
        style: crate::frame::Style { color: false, ascii: true },
        preview: true,
    };
    let fake = Fake::with(&[("describe", ticket()), ("sessions", json!([{"id":"s-1","title":"from jira","updated":7}]))]);
    let plugins = loaded(&fake);
    let extra = |_: &[String]| sessions::collect(&plugins, "", &fake, &mut Vec::new());
    let mut t = crate::ui::PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("Enter").unwrap(), true);
    let got = crate::sessions::run_picker_with(&mut t, &req, &extra).unwrap().unwrap();
    assert_eq!((got.session_id.as_str(), got.transfer), ("s-1", None));
    let frames = t.frames.unwrap();
    assert!(frames.iter().flatten().any(|l| l.contains("[tick]")), "the row names its plugin: {frames:?}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn without_may_run_no_plugin_is_called() {
    let fake = Fake::with(&[("describe", ticket())]);
    let (p, w) = load(&[spec("tick", r"C:\p\tick.exe")], None, &fake, false);
    assert_eq!((p.list.len(), w.len()), (0, 0));
    assert_eq!(fake.calls.lock().unwrap().len(), 0, "preview and --launcher-options start nothing");
}

// --- real processes ---

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cars-plugins-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

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

#[test]
fn a_crash_bad_json_and_a_good_answer_through_real_processes() {
    let d = dir("crash");
    let crash = cmd(&d, "crash", "exit /b 3");
    let bad = cmd(&d, "bad", "echo {not json");
    let good = cmd(&d, "good", r#"echo {"protocol":1}"#);
    assert!(matches!(Process.call(&crash, &describe::VERB, &json!({})), Err(Failure::Exit(3))));
    assert!(matches!(Process.call(&bad, &describe::VERB, &json!({})), Err(Failure::BadJson(_))));
    let (p, w) = load(&[crash, bad, good], None, &Process, true);
    assert_eq!(p.list.iter().map(|l| l.spec.name.as_str()).collect::<Vec<_>>(), ["good"], "the rest goes on");
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(w.first().is_some_and(|l| l.starts_with("plugin crash describe: exit 3")), "{w:?}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_hang_past_its_timeout_is_killed_with_its_tree() {
    let d = dir("hang");
    let mut hang = cmd(&d, "hang", "ping -n 30 127.0.0.1");
    hang.timeout_ms = Some(300);
    let Err(Failure::Timeout { ms, pid }) = Process.call(&hang, &describe::VERB, &json!({})) else {
        panic!("a timeout");
    };
    assert_eq!(ms, 300);
    let left: Vec<_> = crate::sys::process_table().into_iter().filter(|r| r.pid == pid || r.parent == pid).collect();
    assert_eq!(left.len(), 0, "the plugin and its ping are gone: {left:?}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_file_not_listed_never_runs() {
    let d = dir("unlisted");
    let marker = d.join("ran.txt");
    cmd(&d, "evil", &format!("echo x > \"{}\"\r\necho {{\"protocol\":1}}", marker.display()));
    let listed = cmd(&d, "listed", r#"echo {"protocol":1}"#);
    let (p, _) = load(&[listed], None, &Process, true);
    assert_eq!(p.list.len(), 1);
    assert!(!marker.exists(), "a file beside a listed one is never started");
    let missing = spec("cmd", &d.join("cmd.exe").to_string_lossy());
    assert!(
        matches!(Process.call(&missing, &describe::VERB, &json!({})), Err(Failure::Missing)),
        "a missing listed file is never looked up on PATH"
    );
    std::fs::remove_dir_all(&d).unwrap();
}
