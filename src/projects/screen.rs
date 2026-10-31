//! `Invoke-ProjectScreen`: what the project screen alone does around the shared loop - the filter
//! box, the action field stepped from every row, the mode hotkeys, and the pick with its existence
//! check. The free path is read between two loop runs, because only the caller holds the terminal.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::select::{RowKind, project_rows, slugs_for_path};
use super::view::{ProjectView, project_actions, project_frame, step_project_action};
use super::{Project, ProjectChoice};
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, vk};
use crate::launch::LAUNCH_MODES;
use crate::ui::screen_loop::{Cursor, Flow, Screen, run};
use crate::ui::{Exhausted, Hit, Term};

/// What one loop run ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Picked(ProjectChoice),
    /// The free-path row was committed: the caller reads the path and decides.
    AskPath {
        how: String,
    },
}

/// The project screen's own state; the loop's cursor carries the index and the hover.
pub(crate) struct ProjectScreen<'a> {
    projects: &'a [Project],
    cwd: String,
    style: Style,
    clock: &'a dyn Fn() -> i64,
    cursor: Cursor,
    filter: String,
    notice: String,
    action: String,
    hotkeys: Vec<char>,
    rows: Vec<super::select::Row>,
}

impl std::fmt::Debug for ProjectScreen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectScreen")
            .field("cwd", &self.cwd)
            .field("cursor", &self.cursor)
            .field("action", &self.action)
            .finish_non_exhaustive()
    }
}

/// Filter characters besides letters and digits: a pasted path keeps its separators and colon.
const FILTER_EXTRA: [char; 7] = [' ', '-', '.', '_', '\\', '/', ':'];

impl<'a> ProjectScreen<'a> {
    pub(crate) fn new(projects: &'a [Project], cwd: &str, initial_action: &str, style: Style, clock: &'a dyn Fn() -> i64) -> Self {
        let mut hotkeys = vec!['/'];
        hotkeys.extend(LAUNCH_MODES.iter().filter_map(|m| m.hotkey));
        Self {
            projects,
            cwd: cwd.to_owned(),
            style,
            clock,
            cursor: Cursor::at(0),
            filter: String::new(),
            notice: String::new(),
            action: step_project_action(initial_action, 0),
            hotkeys,
            rows: Vec::new(),
        }
    }

    /// R16: a filter parks the cursor on its first match - row 0 when the cwd's project matches,
    /// else the first list row; no filter, or no match, is row 0.
    fn park(&self) -> i32 {
        if self.filter.is_empty() {
            return 0;
        }
        let rows = project_rows(self.projects, &self.filter, &self.cwd);
        if rows.first().is_some_and(|r| r.matched) {
            0
        } else {
            i32::from(rows.iter().any(|r| r.kind == RowKind::Project))
        }
    }

    /// The row under the cursor into a result; every kind is checked for existence, so a row the
    /// registry still lists but whose directory vanished is refused with a notice.
    fn commit(&mut self, how: &str) -> Flow<Outcome> {
        let Some(row) = usize::try_from(self.cursor.index).ok().and_then(|i| self.rows.get(i)) else {
            return Flow::Continue;
        };
        if row.kind == RowKind::Path {
            return Flow::Done(Some(Outcome::AskPath { how: how.to_owned() }));
        }
        let target = row.path.clone();
        if !is_dir(&target) {
            "path not found".clone_into(&mut self.notice);
            return Flow::Continue;
        }
        let slugs = if row.kind == RowKind::Cwd {
            slugs_for_path(self.projects, &target)
        } else {
            row.slugs.clone()
        };
        Flow::Done(Some(Outcome::Picked(choice(target, &self.action, how, slugs))))
    }

    /// The filter box owns every key while it is open. Its two ways of closing are logged with the
    /// LENGTH of the filter, never its text.
    fn type_filter(&mut self, key: &KeyPress, term: &dyn Term) {
        if key.is(vk::ENTER) {
            self.cursor.typing = false;
            self.log(
                term,
                "Enter",
                &[("filter", json!("close")), ("filterLength", json!(self.filter.chars().count()))],
            );
            return;
        }
        if key.is(vk::ESCAPE) || key.is_ctrl(vk::C) {
            self.log(
                term,
                "Escape",
                &[("filter", json!("clear")), ("filterLength", json!(self.filter.chars().count()))],
            );
            self.cursor.typing = false;
            self.filter.clear();
            self.cursor.index = 0;
            return;
        }
        if key.is(vk::BACKSPACE) {
            self.filter.pop();
            self.cursor.index = self.park();
            return;
        }
        if key.ch != '\0' && (key.ch.is_alphanumeric() || FILTER_EXTRA.contains(&key.ch)) {
            self.filter.push(key.ch);
            self.cursor.index = self.park();
        }
    }

    fn log(&self, term: &dyn Term, key: &str, extra: &[(&str, Value)]) {
        let mut d = Map::new();
        d.insert("screen".into(), json!("project"));
        d.insert("key".into(), json!(key));
        d.insert("index".into(), json!(self.cursor.index));
        for (k, v) in extra {
            d.insert((*k).into(), v.clone());
        }
        term.logger().write("key", d);
    }
}

fn is_dir(path: &str) -> bool {
    !path.is_empty() && !path.contains('\0') && Path::new(path).is_dir()
}

fn choice(path: String, action: &str, how: &str, slugs: Vec<String>) -> ProjectChoice {
    ProjectChoice {
        path,
        action: action.to_owned(),
        slug: slugs.first().cloned().unwrap_or_default(),
        slugs,
        how: how.to_owned(),
    }
}

impl Screen for ProjectScreen<'_> {
    type Output = Outcome;

    fn name(&self) -> &'static str {
        "project"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// The filter decides the rows: rebuilt before every frame, the cursor clamped to them.
    fn before(&mut self) {
        self.rows = project_rows(self.projects, &self.filter, &self.cwd);
        let n = i32::try_from(self.rows.len()).unwrap_or(1);
        self.cursor.index = self.cursor.index.min(n - 1).max(0);
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        project_frame(&ProjectView {
            projects: self.projects,
            index: usize::try_from(self.cursor.index).unwrap_or(0),
            filter: &self.filter,
            typing: self.cursor.typing,
            cwd: &self.cwd,
            width: size.0,
            height: size.1,
            now_ms: (self.clock)(),
            style: self.style,
            hover: self.cursor.hover,
            notice: &self.notice,
            action: &self.action,
            hover_row: self.cursor.hover_row,
            hover_value: &self.cursor.hover_value,
        })
    }

    fn rows(&self) -> Option<i32> {
        Some(i32::try_from(self.rows.len()).unwrap_or(0))
    }

    fn has_left_right(&self) -> bool {
        true
    }

    fn left(&mut self) -> Flow<Outcome> {
        self.action = step_project_action(&self.action, -1);
        Flow::Continue
    }

    fn right(&mut self) -> Flow<Outcome> {
        self.action = step_project_action(&self.action, 1);
        Flow::Continue
    }

    fn enter(&mut self) -> Option<Flow<Outcome>> {
        Some(self.commit("enter"))
    }

    fn hotkeys(&self) -> &[char] {
        &self.hotkeys
    }

    /// `/` opens the filter; a mode hotkey sets the field AND commits, so a rejected pick still
    /// shows what the press did.
    fn hotkey(&mut self, c: char) -> Flow<Outcome> {
        if c == '/' {
            self.cursor.typing = true;
            return Flow::Continue;
        }
        if let Some(m) = LAUNCH_MODES.iter().find(|m| m.hotkey == Some(c)) {
            m.name.clone_into(&mut self.action);
            return self.commit("hotkey");
        }
        Flow::Continue
    }

    /// A new KEY retires the last notice (a mouse gesture never does); while typing, the filter
    /// box takes the key.
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        self.notice.clear();
        if self.cursor.typing {
            self.type_filter(key, term);
            return true;
        }
        false
    }

    /// A click on a value walks the field there with the arrows' own stepper; a click on a row
    /// only moves the cursor (a stray click never starts a session).
    fn click(&mut self, hit: &Hit) -> Flow<Outcome> {
        match hit {
            Hit::Action { value: Some(v) } => {
                let values = project_actions();
                let pos = |s: &str| values.iter().position(|x| x.eq_ignore_ascii_case(s));
                if let (Some(from), Some(to)) = (pos(&self.action), pos(v)) {
                    let dir = if to < from { -1 } else { 1 };
                    for _ in 0..from.abs_diff(to) {
                        self.action = step_project_action(&self.action, dir);
                    }
                }
            }
            Hit::ListRow(r) => self.cursor.index = *r,
            Hit::None | Hit::Footer { .. } | Hit::Action { value: None } | Hit::Cell { .. } | Hit::LaunchRow { .. } => {}
        }
        Flow::Continue
    }

    /// A click on the row that already carries the cursor is Enter's gesture: it commits the field.
    fn activate(&mut self, _hit: &Hit) -> Option<Flow<Outcome>> {
        Some(self.commit("mouse"))
    }

    fn log_fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("filterLength".into(), json!(self.filter.chars().count()));
        m
    }
}

/// Runs the screen until a pick or a cancel. A committed free-path row reads the path through
/// `Term::read_line`, logs only its length, and returns to the screen with a notice when the
/// path is no directory.
pub(crate) fn run_screen(
    term: &mut dyn Term,
    projects: &[Project],
    cwd: &Path,
    initial_action: &str,
    style: Style,
    clock: &dyn Fn() -> i64,
) -> Result<Option<ProjectChoice>, Exhausted> {
    let cwd = cwd.to_string_lossy();
    let mut screen = ProjectScreen::new(projects, &cwd, initial_action, style, clock);
    loop {
        match run(&mut screen, term)? {
            None => return Ok(None),
            Some(Outcome::Picked(c)) => return Ok(Some(c)),
            Some(Outcome::AskPath { how }) => {
                let raw = term.read_line("  path: ");
                let typed = raw.trim_matches(['"', ' ']);
                let found = is_dir(typed);
                screen.log(
                    term,
                    "path",
                    &[("pathLength", json!(typed.chars().count())), ("found", json!(found))],
                );
                if found {
                    let full = std::path::absolute(typed).map_or_else(|_| typed.to_owned(), |p| p.to_string_lossy().into_owned());
                    let slugs = slugs_for_path(projects, &full);
                    return Ok(Some(choice(full, &screen.action, &how, slugs)));
                }
                "path not found".clone_into(&mut screen.notice);
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::many_single_char_names, reason = "short fixture names")]
mod tests {
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
        fn wait(&mut self, _s: (usize, usize)) -> Result<Event, Exhausted> {
            self.q.pop_front().ok_or(Exhausted)
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
        let out = run_screen(&mut t, ps, Path::new(cwd), initial, Style { color: false, ascii: true }, &c).unwrap();
        (out, t)
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
        assert!(
            drive_with(&f.projects, &f.cwd, "new", vec![Event::Key(upper_c), key("Escape")], "")
                .0
                .is_none()
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
}
