//! `Invoke-ProjectScreen`: what the project screen alone does around the shared loop - the filter
//! box, the action field stepped from every row, the mode hotkeys, and the pick with its existence
//! check. The free path is read between two loop runs, because only the caller holds the terminal.

use std::cell::RefCell;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::paths::project_key;
use super::select::{RowKind, project_rows, slugs_for_path};
use super::view::{ProjectView, project_actions, project_frame, step_project_action};
use super::{Project, ProjectChoice};
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, is_hotkey_exact, vk};
use crate::launch::LAUNCH_MODES;
use crate::ui::screen_loop::{Cursor, Flow, Screen, run};
use crate::ui::{Hit, Term, UiStop};

/// What one loop run ends with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Picked(ProjectChoice),
    /// The free-path row was committed: the caller reads the path and decides.
    AskPath {
        how: String,
    },
}

/// What one `h` press does to a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideChange {
    Hide,
    Show,
}

/// Projects hidden by hand (config `hiddenProjects`, full paths) and how a change is saved.
pub struct Hiding<'a> {
    pub paths: RefCell<Vec<String>>,
    /// Saves one change for the project at this path; `Err` says why it was not saved.
    pub save: &'a dyn Fn(&str, HideChange) -> Result<(), String>,
}

impl std::fmt::Debug for Hiding<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hiding").field("paths", &self.paths).finish_non_exhaustive()
    }
}

/// `h` hides or shows the project under the cursor, `v` lists the hidden ones marked.
const HIDE_KEY: char = 'h';
const SHOW_HIDDEN_KEY: char = 'v';

/// The project screen's own state; the loop's cursor carries the index and the hover.
pub(crate) struct ProjectScreen<'a> {
    projects: &'a [Project],
    hiding: Option<&'a Hiding<'a>>,
    show_hidden: bool,
    /// `projects` as listed: hand-hidden ones hidden, or every one shown in the show-hidden view.
    view: Vec<Project>,
    /// Keys of the hidden projects the show-hidden view lists; empty otherwise.
    marked: Vec<String>,
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

impl<'a> ProjectScreen<'a> {
    pub(crate) fn new(projects: &'a [Project], cwd: &str, initial_action: &str, style: Style, clock: &'a dyn Fn() -> i64) -> Self {
        let mut hotkeys = vec!['/', SHOW_HIDDEN_KEY];
        hotkeys.extend(LAUNCH_MODES.iter().filter_map(|m| m.hotkey));
        Self {
            projects,
            hiding: None,
            show_hidden: false,
            view: projects.to_vec(),
            marked: Vec::new(),
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

    pub(crate) fn with_hiding(self, hiding: Option<&'a Hiding<'a>>) -> Self {
        Self { hiding, ..self }
    }

    /// The listed projects: a hand-hidden one is hidden like a rule-hidden one; the show-hidden
    /// view lists every one and marks the hidden.
    fn rebuild_view(&mut self) {
        let manual: Vec<String> = self
            .hiding
            .map(|h| h.paths.borrow().iter().map(|p| project_key(p)).collect())
            .unwrap_or_default();
        let hidden = |p: &Project| p.hidden || manual.contains(&project_key(&p.path));
        self.marked = if self.show_hidden {
            self.projects.iter().filter(|p| hidden(p)).map(|p| project_key(&p.path)).collect()
        } else {
            Vec::new()
        };
        self.view = self
            .projects
            .iter()
            .map(|p| Project {
                hidden: !self.show_hidden && hidden(p),
                ..p.clone()
            })
            .collect();
    }

    /// `h`: a listed project joins `hiddenProjects`, a hand-hidden one leaves it; a rule-hidden one
    /// (headless or scratch) stays as it is. Every change is saved at once.
    fn toggle_hide(&mut self) {
        let Some(h) = self.hiding else { return };
        let Some((kind, name, path)) = usize::try_from(self.cursor.index)
            .ok()
            .and_then(|i| self.rows.get(i))
            .map(|r| (r.kind, r.name.clone(), r.path.clone()))
        else {
            return;
        };
        if kind != RowKind::Project {
            "only a listed project can be hidden".clone_into(&mut self.notice);
            return;
        }
        let key = project_key(&path);
        let mut paths = h.paths.borrow_mut();
        let change = if let Some(i) = paths.iter().position(|p| project_key(p) == key) {
            paths.remove(i);
            HideChange::Show
        } else if self.projects.iter().any(|p| p.hidden && project_key(&p.path) == key) {
            "hidden by rule (headless or scratch), not by h".clone_into(&mut self.notice);
            return;
        } else {
            paths.push(path.clone());
            HideChange::Hide
        };
        self.notice = match ((h.save)(&path, change), change) {
            (Ok(()), HideChange::Show) => format!("{name} shown again"),
            (Ok(()), HideChange::Hide) => format!("{name} hidden - v lists hidden projects"),
            (Err(e), HideChange::Show) => format!("{name} shown for this run only: {e}"),
            (Err(e), HideChange::Hide) => format!("{name} hidden for this run only: {e}"),
        };
    }

    /// R16: a filter parks the cursor on its first match - row 0 when the cwd's project matches,
    /// else the first list row; no filter, or no match, is row 0.
    fn park(&self) -> i32 {
        if self.filter.is_empty() {
            return 0;
        }
        let rows = project_rows(&self.view, &self.filter, &self.cwd);
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
        // Any printable char: a Windows folder name may hold `~` (8.3 names), brackets, `+`, `@`.
        if key.ch != '\0' && !key.ch.is_control() {
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
        self.rebuild_view();
        self.rows = project_rows(&self.view, &self.filter, &self.cwd);
        let n = i32::try_from(self.rows.len()).unwrap_or(1);
        self.cursor.index = self.cursor.index.min(n - 1).max(0);
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        project_frame(&ProjectView {
            projects: &self.view,
            marked: &self.marked,
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
        if c == SHOW_HIDDEN_KEY {
            self.show_hidden = !self.show_hidden;
            return Flow::Continue;
        }
        if let Some(m) = LAUNCH_MODES.iter().find(|m| m.hotkey == Some(c)) {
            m.name.clone_into(&mut self.action);
            return self.commit("hotkey");
        }
        Flow::Continue
    }

    /// A new KEY retires the last notice (a mouse gesture never does); while typing, the filter
    /// box takes the key. `h` writes the config, so it is matched exactly: a split mouse report's
    /// Shift + upper-case letter never hides a project.
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        self.notice.clear();
        if self.cursor.typing {
            self.type_filter(key, term);
            return true;
        }
        if is_hotkey_exact(key, HIDE_KEY) {
            self.toggle_hide();
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
    hiding: Option<&Hiding<'_>>,
) -> Result<Option<ProjectChoice>, UiStop> {
    let cwd = cwd.to_string_lossy();
    let mut screen = ProjectScreen::new(projects, &cwd, initial_action, style, clock).with_hiding(hiding);
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
mod tests;
