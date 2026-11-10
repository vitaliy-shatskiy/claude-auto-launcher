#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use super::*;
use crate::input::{Event, MouseEvent};
use crate::projects::registry::tests::Tree;
use crate::projects::select::tests::proj;
use crate::ui::{Activation, PreviewTerm};
use std::collections::VecDeque;

/// Scripted events, a fixed size, a canned answer for the free path, and every frame kept.
struct Script {
    inner: PreviewTerm,
    q: VecDeque<Event>,
    answer: String,
    asked: usize,
}

impl Term for Script {
    fn size(&mut self) -> (usize, usize) {
        (80, 24)
    }
    fn paint(&mut self, lines: &[String]) {
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        self.q.pop_front().ok_or(UiStop::KeysExhausted)
    }
    fn window_top(&mut self) -> i32 {
        0
    }
    fn input_pending(&mut self) -> bool {
        false
    }
    fn record_time(&self) -> Option<u64> {
        None
    }
    fn last_activation(&mut self) -> &mut Option<Activation> {
        self.inner.last_activation()
    }
    fn drain(&mut self) {}
    fn logger(&self) -> &crate::log::Logger {
        self.inner.logger()
    }
    fn read_line(&mut self, _prompt: &str) -> String {
        self.asked += 1;
        self.answer.clone()
    }
}

fn key(k: &str) -> Event {
    Event::Key(crate::input::scripted_key(k).unwrap())
}

fn keys(ks: &[&str]) -> Vec<Event> {
    ks.iter().map(|k| key(k)).collect()
}

fn mouse(x: i32, y: i32, left: bool, is_move: bool) -> Event {
    Event::Mouse(MouseEvent {
        x,
        y,
        left,
        buttons: u32::from(left),
        is_move,
        ..MouseEvent::default()
    })
}

struct Fx {
    tree: Tree,
    alpha: String,
    beta: String,
    cwd: String,
    projects: Vec<Project>,
}

fn fx() -> Fx {
    let t = Tree::new("screen");
    let alpha = t.dir(r"root\alpha");
    let beta = t.dir(r"root\beta");
    let cwd = t.dir("cwd");
    let projects = vec![
        Project {
            last_activity_ms: Some(2_000),
            ..proj("A", &alpha, "alpha")
        },
        Project {
            last_activity_ms: Some(1_000),
            ..proj("B", &beta, "beta")
        },
    ];
    Fx {
        tree: t,
        alpha,
        beta,
        cwd,
        projects,
    }
}

fn clock() -> i64 {
    10_000
}

fn drive_with(ps: &[Project], cwd: &str, initial: &str, events: Vec<Event>, answer: &str) -> (Option<ProjectChoice>, Script) {
    let mut t = Script {
        inner: PreviewTerm::new((80, 24), VecDeque::new(), true),
        q: events.into(),
        answer: answer.into(),
        asked: 0,
    };
    let c = clock;
    let out = run_screen(&mut t, ps, Path::new(cwd), initial, Style { color: false, ascii: true }, &c, None).unwrap();
    (out, t)
}

fn project_frames(f: &Fx, timings: crate::timing::Timings) -> Vec<Vec<String>> {
    let mut t = Script {
        inner: PreviewTerm::new((80, 24), VecDeque::new(), true).with_logger(crate::log::Logger::silent(timings)),
        q: keys(&["s", "Enter"]).into(),
        answer: String::new(),
        asked: 0,
    };
    let c = clock;
    let out = run_screen(
        &mut t,
        &f.projects,
        Path::new(&f.cwd),
        "new",
        Style { color: false, ascii: true },
        &c,
        None,
    )
    .unwrap();
    assert!(out.is_some());
    t.inner.frames.unwrap()
}

#[test]
fn debug_mode_draws_the_project_screen_exactly_as_off() {
    let f = fx();
    let on = crate::timing::Timings::on(std::time::Instant::now());
    assert_eq!(project_frames(&f, on), project_frames(&f, crate::timing::Timings::default()));
}

fn drive(f: &Fx, ks: &[&str]) -> Option<ProjectChoice> {
    drive_with(&f.projects, &f.cwd, "new", keys(ks), "").0
}

/// The map of the frame the screen opens on.
fn opening_map(f: &Fx) -> crate::frame::RowMap {
    let c = clock;
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c);
    s.before();
    s.draw((80, 24)).map
}

/// One launcher run's project screen over `config`: `hiddenProjects` read from the file, every
/// change saved back to it as the launcher saves it.
fn run_with_config(f: &Fx, config: &str, ks: &[&str]) -> Vec<Vec<String>> {
    let ctx = crate::config::PathCtx {
        home: f.tree.0.clone(),
        cwd: f.tree.0.clone(),
    };
    let cfg = crate::config::read_config(config, &ctx);
    let save = |paths: &[String]| {
        crate::config::save_keys(config, &[("hiddenProjects", serde_json::json!(paths))]).unwrap();
    };
    let hiding = Hiding {
        paths: RefCell::new(cfg.hidden_projects),
        save: &save,
    };
    let mut t = Script {
        inner: PreviewTerm::new((80, 24), VecDeque::new(), true),
        q: keys(ks).into(),
        answer: String::new(),
        asked: 0,
    };
    let c = clock;
    let style = Style { color: false, ascii: true };
    assert!(run_screen(&mut t, &f.projects, Path::new(&f.cwd), "new", style, &c, Some(&hiding)).unwrap().is_none());
    t.inner.frames.unwrap()
}

#[test]
fn h_hides_and_unhides_in_show_hidden() {
    let f = fx();
    let config = f.tree.0.join("claude-auto.json");
    let config = config.to_str().unwrap();
    std::fs::write(config, r#"{"remote": false}"#).unwrap();
    // The title line ("N known") carries the notice naming the project: only list rows count.
    let listed = |frame: &[String]| frame.iter().any(|l| l.contains("alpha") && !l.contains("known"));

    let first = run_with_config(&f, config, &["s", "h", "Escape"]);
    assert!(!listed(first.last().unwrap()), "h hides alpha at once");

    let restarted = run_with_config(&f, config, &["Escape"]);
    assert!(!listed(&restarted[0]), "the launcher restarts: alpha stays hidden");
    assert!(std::fs::read_to_string(config).unwrap().contains("\"remote\": false"), "other keys kept");

    let shown = run_with_config(&f, config, &["v", "s", "h", "v", "Escape"]);
    assert!(
        shown[1].iter().any(|l| l.contains("alpha (hidden)")),
        "the show-hidden view lists it marked: {:#?}",
        shown[1]
    );
    assert!(listed(shown.last().unwrap()), "h there un-hides it");
    let again = run_with_config(&f, config, &["Escape"]);
    assert!(listed(&again[0]), "and it stays listed");
}

#[test]
fn h_on_the_current_directory_hides_nothing() {
    let f = fx();
    let config = f.tree.0.join("c.json");
    let config = config.to_str().unwrap();
    let frames = run_with_config(&f, config, &["h", "Escape"]);
    assert!(frames.last().unwrap().iter().any(|l| l.contains("only a listed project")));
    assert!(!Path::new(config).exists(), "nothing saved");
}

#[test]
fn enter_hotkeys_and_escape() {
    let f = fx();
    let p1 = drive(&f, &["s", "Enter"]).unwrap();
    assert_eq!(
        (p1.path.as_str(), p1.action.as_str(), p1.slug.as_str(), p1.how.as_str()),
        (f.alpha.as_str(), "new", "A", "enter")
    );
    let p2 = drive(&f, &["s", "s", "c"]).unwrap();
    assert_eq!(
        (p2.path.as_str(), p2.action.as_str(), p2.slug.as_str()),
        (f.beta.as_str(), "continue", "B")
    );
    assert_eq!(drive(&f, &["r"]).unwrap().action, "resume");
    assert_eq!(drive(&f, &["t"]).unwrap().action, "worktree");
    assert!(drive(&f, &["Escape"]).is_none());
    let p5 = drive(&f, &["Enter"]).unwrap();
    assert_eq!((p5.path.as_str(), p5.slug.as_str()), (f.cwd.as_str(), ""));
    let variant = format!("{}/", f.alpha.replace('\\', "/").to_uppercase());
    assert_eq!(drive_with(&f.projects, &variant, "new", keys(&["Enter"]), "").0.unwrap().slug, "A");
}

#[test]
fn the_filter_owns_the_keys_and_parks_on_its_match() {
    let f = fx();
    assert_eq!(drive(&f, &["/", "e", "t", "a", "Enter", "Enter"]).unwrap().path, f.beta);
    assert!(drive(&f, &["/", "c", "Escape", "Escape"]).is_none(), "c edits the filter");
    assert_eq!(
        drive(&f, &["/", "z", "Escape", "Enter"]).unwrap().path,
        f.cwd,
        "first Escape clears"
    );
    assert!(drive(&f, &["/", "z", "Escape", "Escape"]).is_none());
    let mut chars: Vec<String> = vec!["/".into()];
    chars.extend(f.alpha.chars().map(String::from));
    chars.extend(["Enter".into(), "Enter".into()]);
    let ks: Vec<&str> = chars.iter().map(String::as_str).collect();
    assert_eq!(drive(&f, &ks).unwrap().path, f.alpha, "a full path as the filter");
}

#[test]
fn the_filter_keeps_short_name_and_bracket_chars() {
    let t = Tree::new("screen-odd");
    let odd = t.dir(r"RUNNER~1\app (x86)+c@home");
    let cwd = t.dir("cwd");
    let projects = vec![proj("O", &odd, "odd")];
    let mut ks: Vec<String> = vec!["/".into()];
    ks.extend(odd.chars().map(String::from));
    ks.extend(["Enter".into(), "Enter".into()]);
    let refs: Vec<&str> = ks.iter().map(String::as_str).collect();
    let picked = drive_with(&projects, &cwd, "new", keys(&refs), "").0;
    assert_eq!(picked.map(|p| p.path), Some(odd), "a path with ~, brackets, + and @ as the filter");
}

#[test]
fn a_cwd_that_is_a_project_runs_there_and_parks_first() {
    let f = fx();
    let p = drive_with(&f.projects, &f.beta, "new", keys(&["Enter"]), "").0.unwrap();
    assert_eq!((p.path.as_str(), p.slug.as_str()), (f.beta.as_str(), "B"));
    assert_eq!(
        drive_with(&f.projects, &f.beta, "new", keys(&["s", "Enter"]), "").0.unwrap().path,
        f.alpha
    );
    let betamax = f.tree.dir(r"root\betamax");
    let mut ps = f.projects.clone();
    ps.push(proj("BM", &betamax, "betamax"));
    let parked = drive_with(&ps, &f.beta, "new", keys(&["/", "e", "t", "a", "Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!(parked.path, f.beta, "the cwd row's match wins the park");
    let other = drive_with(&f.projects, &f.beta, "new", keys(&["/", "a", "l", "p", "Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!(other.path, f.alpha);
}

#[test]
fn the_free_path_is_read_checked_and_resolved() {
    let f = fx();
    let (miss, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        keys(&["s", "s", "s", "Enter", "Escape"]),
        r"C:\this-path-does-not-really-exist-9f3a",
    );
    assert!(miss.is_none());
    assert_eq!(t.asked, 1);
    let scratch = f.tree.dir("free");
    let hit = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        keys(&["s", "s", "s", "Enter", "Escape"]),
        &format!("\"{scratch}\""),
    )
    .0
    .unwrap();
    assert_eq!((hit.path.as_str(), hit.slug.as_str()), (scratch.as_str(), ""));
    let resumed = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        keys(&["RightArrow", "RightArrow", "s", "s", "s", "Enter", "Escape"]),
        &scratch,
    )
    .0
    .unwrap();
    assert_eq!((resumed.action.as_str(), resumed.path.as_str()), ("resume", scratch.as_str()));
    let d = drive_with(&f.projects, &f.cwd, "new", keys(&["s", "s", "s", "d", "Enter"]), &scratch)
        .0
        .unwrap();
    assert_eq!(d.action, "continue", "d steps the field on the free-path row too");
}

#[test]
fn a_vanished_row_is_refused_and_the_hotkey_still_sets_the_field() {
    let f = fx();
    let gone = f.tree.0.join("vanished").to_string_lossy().into_owned();
    let ps = vec![proj("V", &gone, "vanished")];
    assert!(drive_with(&ps, &f.cwd, "new", keys(&["s", "Enter", "Escape"]), "").0.is_none());
    for (k, want) in [("t", "worktree"), ("c", "continue"), ("r", "resume")] {
        let (out, t) = drive_with(&ps, &gone, "new", keys(&[k, "Escape"]), "");
        assert!(out.is_none());
        let frames = t.inner.frames.unwrap();
        let second = frames[1].join("\n");
        assert!(second.contains(&format!("[{want}]")), "{k}: the field the press left behind");
        assert!(second.contains("path not found"), "{k}: the notice");
    }
}

#[test]
fn the_action_field_steps_from_any_row_and_wraps() {
    let f = fx();
    let a1 = drive(&f, &["s", "RightArrow", "Enter"]).unwrap();
    assert_eq!((a1.action.as_str(), a1.path.as_str()), ("continue", f.alpha.as_str()));
    assert_eq!(drive(&f, &["RightArrow", "RightArrow", "Enter"]).unwrap().action, "resume");
    assert_eq!(
        drive(&f, &["RightArrow", "RightArrow", "RightArrow", "RightArrow", "Enter"])
            .unwrap()
            .action,
        "new"
    );
    assert_eq!(drive(&f, &["LeftArrow", "Enter"]).unwrap().action, "worktree");
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "resume", keys(&["Enter"]), "").0.unwrap().action,
        "resume"
    );
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "rm -rf", keys(&["Enter"]), "").0.unwrap().action,
        "new"
    );
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "RESUME", keys(&["RightArrow", "Enter"]), "")
            .0
            .unwrap()
            .action,
        "worktree"
    );
    let d = drive(&f, &["s", "d", "Enter"]).unwrap();
    assert_eq!((d.action.as_str(), d.path.as_str()), ("continue", f.alpha.as_str()));
    assert_eq!(drive(&f, &["s", "a", "Enter"]).unwrap().action, "worktree");
}

#[test]
fn down_stops_at_the_free_path_row() {
    let f = fx();
    let (out, t) = drive_with(&f.projects, &f.cwd, "new", keys(&["s", "s", "s", "s", "s", "w", "Escape"]), "");
    assert!(out.is_none());
    let frames = t.inner.frames.unwrap();
    assert_eq!(frames.len(), 7);
    let cursor_line = |i: usize| frames[i].iter().position(|l| l.contains(" > ")).unwrap();
    assert_eq!(cursor_line(3), cursor_line(4));
    assert_eq!(cursor_line(4), cursor_line(5));
    assert!(cursor_line(6) < cursor_line(5));
}

#[test]
fn mouse_click_moves_second_click_commits_and_values_walk() {
    let f = fx();
    let map = opening_map(&f);
    let ys = map.list.clone().unwrap().row_ys;
    let (one, _) = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, ys[1], true, false), key("Escape")], "");
    assert!(one.is_none(), "one click starts nothing");
    let moved = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, ys[1], true, false), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(moved.path, f.alpha);
    let twice = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![mouse(4, ys[1], true, false), mouse(4, ys[1], true, false), key("Escape")],
        "",
    )
    .0
    .unwrap();
    assert_eq!((twice.path.as_str(), twice.how.as_str()), (f.alpha.as_str(), "mouse"));
    let after_gap = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, ys[2], true, false), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(after_gap.path, f.beta, "the gap is not hit-testable");
    let a = map.action.unwrap();
    let pick = |x: i32| {
        drive_with(&f.projects, &f.cwd, "new", vec![mouse(x, a.y, true, false), key("Enter")], "")
            .0
            .unwrap()
    };
    let cont = pick(a.cells[1].start);
    assert_eq!((cont.action.as_str(), cont.path.as_str()), ("continue", f.cwd.as_str()));
    assert_eq!(pick(a.cells[3].end).action, "worktree");
    assert_eq!(pick(1).action, "new", "between the cells changes nothing");
    let detail = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, a.y - 1, true, false), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(detail.path, f.cwd, "the detail line is inert");
    let committed = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![
            key("RightArrow"),
            mouse(4, ys[1], true, false),
            mouse(4, ys[1], true, false),
            key("Escape"),
        ],
        "",
    )
    .0
    .unwrap();
    assert_eq!((committed.action.as_str(), committed.path.as_str()), ("continue", f.alpha.as_str()));
}

#[test]
fn hover_paints_never_selects_and_a_notice_outlives_the_mouse() {
    let f = fx();
    let map = opening_map(&f);
    let ys = map.list.clone().unwrap().row_ys;
    let p = drive_with(&f.projects, &f.cwd, "new", vec![mouse(3, ys[2], false, true), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(p.path, f.cwd, "hover leaves the cursor");
    let (_, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![mouse(3, ys[0], false, true), mouse(4, ys[0], false, true), key("Escape")],
        "",
    );
    assert_eq!(t.inner.frames.unwrap().len(), 2, "a second identical move draws nothing");
    let a = map.action.unwrap();
    let (_, t2) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![mouse(3, a.y, false, true), mouse(4, a.y, false, true), key("Escape")],
        "",
    );
    assert_eq!(
        t2.inner.frames.unwrap().len(),
        1,
        "the action row outside a value is no hover change"
    );
    let fy = map.footer_y.unwrap();
    let c = map.footer.iter().find(|s| s.ch == Some('c')).unwrap();
    let events = vec![
        key("s"),
        key("s"),
        key("s"),
        key("Enter"),
        mouse(c.start, fy + i32::try_from(c.line).unwrap(), false, true),
        key("w"),
        key("Escape"),
    ];
    let (_, t3) = drive_with(&f.projects, &f.cwd, "new", events, r"C:\bogus-notice-persists-9f3a");
    let frames: Vec<String> = t3.inner.frames.unwrap().iter().map(|l| l.join("\n")).collect();
    let n = frames.len();
    assert!(
        frames[n - 3].contains("path not found") && frames[n - 2].contains("path not found"),
        "a move keeps it"
    );
    assert!(!frames[n - 1].contains("path not found"), "the next key clears it");
}

#[test]
fn wheel_ctrl_c_and_an_uppercase_hotkey() {
    let f = fx();
    let down = Event::Mouse(MouseEvent {
        wheel: -128,
        ..MouseEvent::default()
    });
    let up = Event::Mouse(MouseEvent {
        wheel: 128,
        ..MouseEvent::default()
    });
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "new", vec![down, key("Enter")], "").0.unwrap().path,
        f.alpha
    );
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "new", vec![down, up, key("Enter")], "")
            .0
            .unwrap()
            .path,
        f.cwd
    );
    let ctrl_c = KeyPress {
        vk: vk::C,
        ctrl: true,
        ch: '\u{3}',
        ..KeyPress::default()
    };
    assert!(drive_with(&f.projects, &f.cwd, "new", vec![Event::Key(ctrl_c)], "").0.is_none());
    let upper_c = KeyPress {
        vk: vk::C,
        ch: 'C',
        ..KeyPress::default()
    };
    let picked = drive_with(&f.projects, &f.cwd, "new", vec![Event::Key(upper_c), key("Escape")], "").0;
    assert_eq!(
        picked.map(|p| p.action),
        Some("continue".to_owned()),
        "Caps Lock's 'C' (psmux drops the flag) is still the continue hotkey"
    );
}

#[test]
fn hidden_rows_are_reached_through_the_filter() {
    let f = fx();
    let probe = f.tree.dir(r"temp\probe-1");
    let job = f.tree.dir(r"p\jobs\1\tmp\w585");
    let mut ps = f.projects.clone();
    ps.push(Project {
        hidden: true,
        ..proj("T", &probe, "probe-1")
    });
    ps.push(Project {
        hidden: true,
        ..proj("J", &job, "w585")
    });
    let picked = drive_with(&ps, &f.cwd, "new", keys(&["/", "w", "5", "8", "Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!((picked.path.as_str(), picked.slug.as_str()), (job.as_str(), "J"));
    let stepped = drive_with(&ps, &f.cwd, "new", keys(&["s", "s", "s", "Enter"]), &probe).0.unwrap();
    assert_eq!(stepped.path, probe, "the unfiltered list does not show the hidden rows");
}
