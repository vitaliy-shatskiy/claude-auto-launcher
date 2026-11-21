//! The launch page as a workspace on a wide terminal: the launch rows, the project list and the
//! selected project's sessions are panels, and Tab, Shift+Tab or a click moves the focus between
//! them; the focused panel takes the keys. Under two columns the page keeps the recent panel and
//! the project page follows on Enter.

use super::LaunchScreen;
use crate::cockpit;
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

    /// The focus, when the page is the workspace (the list is in and two columns fit).
    pub(super) fn home_focus(&self) -> Option<HomeFocus> {
        (self.home.is_some() && self.width >= cockpit::TWO_COLUMNS).then_some(self.focus)
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
        f
    }
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
