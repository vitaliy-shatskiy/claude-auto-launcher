//! What the project screen alone does around the shared loop - the filter
//! box, the action field stepped from every row, the mode hotkeys, the pick with its existence
//! check, and the path navigator the free-path row opens in the same screen.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::detail::{Detail, DetailSource};
use super::nav_view::{NavView, navigator_frame};
use super::navigator::{Lister, NETWORK_LIMIT, Navigator, list_dirs};
use super::paths::DriveCache;
use super::select::project_rows;
use super::view::{ProjectView, project_actions, project_frame, step_project_action};
use super::{Project, ProjectChoice};
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, is_hotkey_exact};
use crate::launch::LAUNCH_MODES;
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

/// What one `h` or `p` press does to a project. Hide also unpins and Pin also unhides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideChange {
    Hide,
    Show,
    Pin,
    Unpin,
}

/// Projects hidden (config `hiddenProjects`) and pinned (`pinnedProjects`) by hand, full paths,
/// and how a change is saved.
pub struct Hiding<'a> {
    pub paths: RefCell<Vec<String>>,
    /// Pinned on top in this order.
    pub pinned: RefCell<Vec<String>>,
    /// Saves one change for the project at this path; `Err` says why it was not saved.
    pub save: &'a dyn Fn(&str, HideChange) -> Result<(), String>,
}

impl std::fmt::Debug for Hiding<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hiding")
            .field("paths", &self.paths)
            .field("pinned", &self.pinned)
            .finish_non_exhaustive()
    }
}

/// `h` hides or shows the project under the cursor, `p` pins or unpins it, `v` lists the hidden
/// ones marked.
const HIDE_KEY: char = 'h';
const PIN_KEY: char = 'p';
const SHOW_HIDDEN_KEY: char = 'v';

/// The project screen's own state; the loop's cursor carries the index and the hover.
pub(crate) struct ProjectScreen<'a> {
    /// Owned: the launch page's projects panel holds the screen beside the list it loaded.
    projects: Vec<Project>,
    hiding: Option<&'a Hiding<'a>>,
    show_hidden: bool,
    /// `projects` as listed: hand-hidden ones hidden, or every one shown in the show-hidden view.
    view: Vec<Project>,
    /// Keys of the hidden projects the show-hidden view lists; empty otherwise.
    marked: Vec<String>,
    /// Keys of the pins whose folder is gone, drawn dimmed (AC5).
    dimmed: Vec<String>,
    /// Keys of the pinned projects, drawn with a pin mark.
    pin_keys: Vec<String>,
    /// Folder checks of the pins the registry does not list, once per key per run.
    absent: BTreeMap<String, bool>,
    /// The project a `p` moved: the cursor follows it on the next frame.
    follow: Option<String>,
    cwd: String,
    style: Style,
    clock: &'a dyn Fn() -> i64,
    cursor: Cursor,
    filter: String,
    notice: String,
    action: String,
    hotkeys: Vec<char>,
    rows: Vec<super::select::Row>,
    /// The open path navigator; the list's cursor waits in `list_index`.
    nav: Option<Navigator>,
    list_index: i32,
    lister: Lister,
    limit: Duration,
    drives: DriveCache,
    preview: bool,
    /// Where the selected project's sessions come from; None draws the context as loading.
    details: Option<DetailSource>,
    detail: Option<Detail>,
    /// Set while the launch page holds the screen as its projects panel.
    pub(crate) home: Option<super::view::Home>,
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
    pub(crate) fn new(projects: &[Project], cwd: &str, initial_action: &str, style: Style, clock: &'a dyn Fn() -> i64) -> Self {
        let mut hotkeys = vec!['/', SHOW_HIDDEN_KEY];
        hotkeys.extend(LAUNCH_MODES.iter().filter_map(|m| m.hotkey));
        Self {
            projects: projects.to_vec(),
            hiding: None,
            show_hidden: false,
            view: projects.to_vec(),
            marked: Vec::new(),
            dimmed: Vec::new(),
            pin_keys: Vec::new(),
            absent: BTreeMap::new(),
            follow: None,
            cwd: cwd.to_owned(),
            style,
            clock,
            cursor: Cursor::at(0),
            filter: String::new(),
            notice: String::new(),
            action: step_project_action(initial_action, 0),
            hotkeys,
            rows: Vec::new(),
            nav: None,
            list_index: 0,
            lister: list_dirs,
            limit: NETWORK_LIMIT,
            drives: DriveCache::new(),
            preview: false,
            details: None,
            detail: None,
            home: None,
        }
    }

    pub(crate) fn with_hiding(self, hiding: Option<&'a Hiding<'a>>) -> Self {
        Self { hiding, ..self }
    }

    /// The selected project's sessions and branch, from source.
    pub(crate) fn with_details(self, details: Option<DetailSource>) -> Self {
        Self { details, ..self }
    }

    /// Asks for the selected row's detail; an in-place source answers at once.
    fn ask_detail(&mut self) -> bool {
        let (Some(src), None) = (self.details.as_mut(), self.nav.as_ref()) else {
            return false;
        };
        let Some(r) = self.rows.get(usize::try_from(self.cursor.index).unwrap_or(0)) else {
            return false;
        };
        let slugs = if r.slugs.is_empty() {
            vec![r.slug.clone()]
        } else {
            r.slugs.clone()
        };
        let mut changed = false;
        if let Some(d) = src.ask(&r.path, &slugs) {
            self.detail = Some(d);
            changed = true;
        }
        if let Some(d) = src.poll() {
            self.detail = Some(d);
            changed = true;
        }
        changed
    }

    /// Preview mode: the navigator never creates a folder.
    pub(crate) fn with_preview(self, preview: bool) -> Self {
        Self { preview, ..self }
    }
}

impl Screen for ProjectScreen<'_> {
    type Output = ProjectChoice;

    fn name(&self) -> &'static str {
        "project"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    /// The filter (or the navigator's text) decides the rows: rebuilt before every frame, the
    /// cursor clamped to them.
    fn before(&mut self) {
        let n = if let Some(nav) = self.nav.as_mut() {
            if nav.refresh() {
                self.cursor.index = i32::try_from(nav.park()).unwrap_or(0);
            }
            nav.rows.len()
        } else {
            self.rebuild_view();
            self.rows = project_rows(&self.view, &self.filter, &self.cwd);
            self.follow_moved();
            self.rows.len()
        };
        let n = i32::try_from(n).unwrap_or(1);
        self.cursor.index = self.cursor.index.min(n - 1).max(0);
        // Preview never idles: its source answers here, in place.
        if matches!(self.details, Some(DetailSource::InPlace { .. })) {
            self.ask_detail();
        }
    }

    /// The cursor rested: the worker is asked for its row (a held key asks only once it stops),
    /// and a finished detail redraws.
    fn idle(&mut self) -> bool {
        self.ask_detail()
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        if let Some(nav) = self.nav.as_ref() {
            return navigator_frame(&NavView {
                nav,
                index: usize::try_from(self.cursor.index).unwrap_or(0),
                width: size.0,
                height: size.1,
                style: self.style,
                action: &self.action,
                hover: self.cursor.hover,
                hover_row: self.cursor.hover_row,
                hover_value: &self.cursor.hover_value,
            });
        }
        project_frame(&ProjectView {
            projects: &self.view,
            marked: &self.marked,
            dimmed: &self.dimmed,
            pinned: &self.pin_keys,
            index: usize::try_from(self.cursor.index).unwrap_or(0),
            filter: &self.filter,
            typing: self.cursor.typing,
            show_hidden: self.show_hidden,
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
            detail: self.detail.as_ref(),
            home: self.home,
        })
    }

    fn rows(&self) -> Option<i32> {
        let n = self.nav.as_ref().map_or(self.rows.len(), |nav| nav.rows.len());
        Some(i32::try_from(n).unwrap_or(0))
    }

    fn up(&mut self) -> bool {
        self.drop_question();
        false
    }

    fn down(&mut self) -> bool {
        self.drop_question();
        false
    }

    fn wheel(&mut self, _delta: i32) -> bool {
        self.drop_question();
        false
    }

    fn has_left_right(&self) -> bool {
        true
    }

    /// Left steps the field back; in the navigator it goes one folder up.
    fn left(&mut self) -> Flow<ProjectChoice> {
        match self.nav.as_mut() {
            Some(nav) => nav.up(),
            None => self.action = step_project_action(&self.action, -1),
        }
        Flow::Continue
    }

    /// Right steps the field on; in the navigator it enters the folder under the cursor.
    fn right(&mut self) -> Flow<ProjectChoice> {
        let index = usize::try_from(self.cursor.index).unwrap_or(0);
        match self.nav.as_mut() {
            Some(nav) => nav.right(index),
            None => self.action = step_project_action(&self.action, 1),
        }
        Flow::Continue
    }

    fn enter(&mut self) -> Option<Flow<ProjectChoice>> {
        if self.nav.is_some() {
            return Some(self.nav_enter("enter"));
        }
        Some(self.commit("enter"))
    }

    /// Esc steps back one level: the navigator's create question, then the navigator itself
    /// (never a launch, no notice), a kept filter, the hidden view, and only then the screen (an
    /// open filter is cleared by the filter box itself).
    fn escape(&mut self) -> Option<Flow<ProjectChoice>> {
        if let Some(nav) = self.nav.as_mut() {
            if !nav.escape() {
                self.nav = None;
                self.cursor.index = self.list_index;
            }
            return Some(Flow::Continue);
        }
        if !self.filter.is_empty() {
            self.filter.clear();
            self.cursor.index = 0;
            return Some(Flow::Continue);
        }
        if self.show_hidden {
            self.show_hidden = false;
            return Some(Flow::Continue);
        }
        None
    }

    /// Tab completes in the navigator; the list has no Tab.
    fn tab(&mut self) -> Option<Flow<ProjectChoice>> {
        let nav = self.nav.as_mut()?;
        nav.tab();
        Some(Flow::Continue)
    }

    fn hotkeys(&self) -> &[char] {
        &self.hotkeys
    }

    /// `/` opens the filter; a mode hotkey sets the field AND commits, so a rejected pick still
    /// shows what the press did.
    fn hotkey(&mut self, c: char) -> Flow<ProjectChoice> {
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

    /// A new KEY retires the last notice (a mouse gesture never does); the navigator or the open
    /// filter box takes the key. `h` and `p` write the config, so they are matched exactly: a split
    /// mouse report's Shift + upper-case letter never hides or pins a project.
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        self.notice.clear();
        if self.nav.is_some() {
            return self.nav_key(key);
        }
        if self.cursor.typing {
            return self.type_filter(key, term);
        }
        if is_hotkey_exact(key, HIDE_KEY) {
            self.toggle_hide();
            return true;
        }
        if is_hotkey_exact(key, PIN_KEY) {
            self.toggle_pin();
            return true;
        }
        false
    }

    /// A click on a value walks the field there with the arrows' own stepper; a click on a row
    /// only moves the cursor (a stray click never starts a session).
    fn click(&mut self, hit: &Hit) -> Flow<ProjectChoice> {
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
            Hit::ListRow(r) => {
                self.drop_question();
                if self.rows().is_some_and(|n| *r < n) {
                    self.cursor.index = *r;
                }
            }
            Hit::None | Hit::Footer { .. } | Hit::Action { value: None } | Hit::Cell { .. } | Hit::LaunchRow { .. } => {}
        }
        Flow::Continue
    }

    /// A second click on the row a click selected is Enter's gesture, but never the create
    /// question's yes: a click while it shows only drops it.
    fn activate(&mut self, _hit: &Hit, _term: &mut dyn Term) -> Option<Flow<ProjectChoice>> {
        if self.drop_question() {
            return Some(Flow::Continue);
        }
        if self.nav.is_some() {
            return Some(self.nav_enter("mouse"));
        }
        Some(self.commit("mouse"))
    }

    fn log_fields(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("filterLength".into(), json!(self.filter.chars().count()));
        if let Some(nav) = self.nav.as_ref() {
            m.insert("pathLength".into(), json!(nav.text.chars().count()));
        }
        m
    }
}

mod filter;
mod marks;
mod pick;
#[cfg(test)]
mod tests;
