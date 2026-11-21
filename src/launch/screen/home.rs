//! The launch page as a workspace on a wide terminal: the launch rows, the project list and the
//! selected project's sessions are panels, and Tab, Shift+Tab or a click moves the focus between
//! them; the focused panel takes the keys. Under two columns the same panels are pages, in the
//! same order.

use super::LaunchScreen;
use crate::frame::Frame;
use crate::input::{KeyPress, vk};
use crate::launch::LaunchState;
use crate::launch::frame::HomePanel;
use crate::projects::ProjectChoice;
use crate::projects::screen::ProjectScreen;
use crate::projects::view::{Home, HomeFocus};
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

/// The project list once the registry is in; `None` while it is still being read.
pub(crate) type HomeSource<'a> = dyn Fn() -> Option<ProjectScreen<'a>> + 'a;

impl<'a> LaunchScreen<'a> {
    /// Where the project list comes from: `idle` asks until it answers.
    #[must_use]
    pub(crate) fn with_home(mut self, source: &'a HomeSource<'a>) -> Self {
        self.home_source = Some(source);
        self.take_home();
        self
    }

    /// `true` when the project list just arrived.
    pub(super) fn take_home(&mut self) -> bool {
        let Some(screen) = self.home_source.and_then(|s| s()) else {
            return false;
        };
        self.home_source = None;
        self.home = Some(screen);
        true
    }

    /// The focus, once the list is in: panels on a wide page, pages on a narrow one.
    pub(super) fn home_focus(&self) -> Option<HomeFocus> {
        self.home.is_some().then_some(self.focus)
    }

    /// The cursor the loop moves: the focused panel's.
    pub(super) fn focused_cursor(&mut self) -> &mut Cursor {
        match (self.home_focus(), self.home.as_mut()) {
            (Some(HomeFocus::Projects), Some(h)) => h.cursor(),
            (Some(HomeFocus::Sessions(_)), _) => &mut self.session_cursor,
            _ => &mut self.cursor,
        }
    }

    /// The focus one step on (`back`: one step back): launch, projects, sessions.
    pub(super) fn cycle(&mut self, back: bool) {
        let order = [HomeFocus::Launch, HomeFocus::Projects, HomeFocus::Sessions(0)];
        let at = order.iter().position(|f| same(*f, self.focus)).unwrap_or(0);
        let next = if back {
            (at + order.len() - 1) % order.len()
        } else {
            (at + 1) % order.len()
        };
        self.focus_on(order.get(next).copied().unwrap_or(HomeFocus::Launch));
    }

    pub(super) fn focus_on(&mut self, f: HomeFocus) {
        if let HomeFocus::Sessions(_) = f {
            self.session_cursor = Cursor::at(0);
        }
        self.focus = f;
    }

    /// The state the launch ends with after the list picked `c`.
    fn picked(&self, c: ProjectChoice, session: Option<(String, bool)>) -> Flow<LaunchState> {
        let mut st = self.state.clone();
        st.home_pick = Some(c);
        st.home_session = session;
        Flow::Done(Some(st))
    }

    /// A list flow as the launch page's: a pick ends it with the launch rows' choices.
    pub(super) fn list_flow(&self, f: Flow<ProjectChoice>) -> Flow<LaunchState> {
        match f {
            Flow::Done(Some(c)) => self.picked(c, None),
            Flow::Done(None) | Flow::Continue => Flow::Continue,
        }
    }

    /// Enter (`fork` false) or `f` on the sessions panel: that session of the selected project.
    pub(super) fn resume(&mut self, fork: bool) -> Flow<LaunchState> {
        let i = usize::try_from(self.session_cursor.index).unwrap_or(0);
        let Some(h) = self.home.as_mut() else { return Flow::Continue };
        let Some(id) = h.sessions().get(i).map(|s| s.session_id.clone()) else {
            return Flow::Continue;
        };
        match h.resume_here("enter") {
            Some(c) => self.picked(c, Some((id, fork))),
            None => Flow::Continue,
        }
    }

    /// A sessions-panel key: `f` forks the selected session, `v` opens it in the session picker
    /// (its chat beside it).
    pub(super) fn session_key(&mut self, c: char) -> Flow<LaunchState> {
        match c {
            'f' => self.resume(true),
            'v' => self.preview_session(),
            _ => Flow::Continue,
        }
    }

    /// The selected session in its project's picker: the chat preview lives there.
    fn preview_session(&mut self) -> Flow<LaunchState> {
        let i = usize::try_from(self.session_cursor.index).unwrap_or(0);
        let Some(h) = self.home.as_mut() else { return Flow::Continue };
        let Some(id) = h.sessions().get(i).map(|s| s.session_id.clone()) else {
            return Flow::Continue;
        };
        match h.resume_here("enter") {
            Some(c) => {
                let mut st = self.state.clone();
                st.home_pick = Some(c);
                st.home_preview = Some(id);
                Flow::Done(Some(st))
            }
            None => Flow::Continue,
        }
    }

    /// The second click on the session a click selected resumes it.
    pub(super) fn session_activate(&mut self, hit: &Hit) -> Option<Flow<LaunchState>> {
        let Hit::SessionRow(i) = *hit else { return None };
        let selected =
            matches!(self.home_focus(), Some(HomeFocus::Sessions(_))) && usize::try_from(self.session_cursor.index).ok() == Some(i);
        selected.then(|| self.resume(false))
    }

    pub(super) fn session_count(&self) -> usize {
        self.home.as_ref().map_or(0, |h| h.sessions().len())
    }

    /// Esc on the workspace: the list's own levels first (filter, navigator, hidden view), then
    /// the focus one panel back toward the launch rows; `None` on the launch rows (quit).
    pub(super) fn home_escape(&mut self) -> Option<Flow<LaunchState>> {
        match self.home_focus()? {
            HomeFocus::Launch => None,
            HomeFocus::Sessions(_) => {
                self.focus = HomeFocus::Projects;
                Some(Flow::Continue)
            }
            HomeFocus::Projects => {
                let inner = self.home.as_mut().and_then(Screen::escape);
                if inner.is_none() {
                    self.focus = HomeFocus::Launch;
                }
                Some(Flow::Continue)
            }
        }
    }

    /// A click: on a list row or the action row it focuses the list, on a launch row the rows.
    pub(super) fn home_click(&mut self, hit: &Hit) -> Option<Flow<LaunchState>> {
        self.home_focus()?;
        match hit {
            Hit::ListRow(_) | Hit::Action { .. } => {
                self.focus = HomeFocus::Projects;
                let f = self.home.as_mut().map(|h| h.click(hit))?;
                Some(self.list_flow(f))
            }
            Hit::SessionRow(i) => {
                self.focus_on(HomeFocus::Sessions(0));
                self.session_cursor.index = i32::try_from(*i).unwrap_or(0);
                Some(Flow::Continue)
            }
            Hit::LaunchRow { .. } | Hit::Cell { .. } => {
                self.focus = HomeFocus::Launch;
                None
            }
            Hit::None | Hit::Footer { .. } => Some(Flow::Continue),
        }
    }

    /// Shift+Tab moves the focus back; the list takes its keys (the caller's hook first unless
    /// a filter or the navigator is open).
    pub(super) fn home_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> Option<bool> {
        let focus = self.home_focus()?;
        if key.is(vk::TAB) && key.shift {
            self.cycle(true);
            return Some(true);
        }
        let busy = self.home.as_ref().is_some_and(ProjectScreen::busy);
        if focus == HomeFocus::Launch || (!busy && (self.on_key)(key, term)) {
            return None;
        }
        match focus {
            HomeFocus::Projects => Some(self.home.as_mut().is_some_and(|h| h.on_key(key, term))),
            HomeFocus::Sessions(_) | HomeFocus::Launch => Some(false),
        }
    }

    /// The page with the workspace's panels: the list drawn at the layout the launch frame asks.
    pub(super) fn home_draw(&mut self, size: (usize, usize)) -> Frame {
        let focus = match self.focus {
            HomeFocus::Sessions(_) => HomeFocus::Sessions(usize::try_from(self.session_cursor.index).unwrap_or(0)),
            f => f,
        };
        let Some(mut list) = self.home.take() else {
            return self.frame_with(size, None);
        };
        list.before();
        let cell = std::cell::RefCell::new(list);
        let paint = |home: Home, w: usize, h: usize| {
            let mut l = cell.borrow_mut();
            l.home = Some(home);
            l.draw((w, h))
        };
        let f = self.frame_with(size, Some(HomePanel { focus, draw: &paint }));
        self.home = Some(cell.into_inner());
        self.seen = Seen {
            list: f.map.list.as_ref().is_some_and(|l| l.row_count > 0),
            sessions: f.map.side.as_ref().map_or(0, |s| s.1.len()),
        };
        f
    }

    /// Whether the last frame showed the focused panel's selected row: Enter and the panel's keys
    /// act only on a row the owner saw (a short or tiny terminal draws no list).
    pub(super) fn row_seen(&self) -> bool {
        match self.home_focus() {
            None | Some(HomeFocus::Launch) => true,
            Some(HomeFocus::Projects) => self.seen.list,
            Some(HomeFocus::Sessions(_)) => usize::try_from(self.session_cursor.index).is_ok_and(|i| i < self.seen.sessions),
        }
    }
}

/// What the last frame showed of the workspace: the list's rows, how many sessions.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Seen {
    list: bool,
    sessions: usize,
}
/// Two focuses are the same panel (the sessions cursor aside).
fn same(a: HomeFocus, b: HomeFocus) -> bool {
    matches!(
        (a, b),
        (HomeFocus::Launch, HomeFocus::Launch)
            | (HomeFocus::Projects, HomeFocus::Projects)
            | (HomeFocus::Sessions(_), HomeFocus::Sessions(_))
    )
}
