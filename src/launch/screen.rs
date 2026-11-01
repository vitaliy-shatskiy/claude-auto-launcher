//! `Invoke-LaunchScreen`: what the launch screen alone does around the shared loop - stepping a
//! row over its visible values, the account row as a tab strip, click-to-value, ctrl+r, and the
//! caller's own key hook (`u` opens maintenance).

use std::collections::BTreeMap;

use super::frame::{LaunchFrameInput, launch_frame};
use super::{LaunchState, OptionText, Roster, RowName, availability_snap, is_hidden_row_value, step_launch_value, visible_row_values};
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, vk};
use crate::prefs::{self, Prefs};
use crate::types::{DefaultLabels, Limits, VersionInfo};
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

/// The caller's hook: `true` consumed the key (the loop redraws).
pub type OnKey<'a> = dyn FnMut(&KeyPress, &mut dyn Term) -> bool + 'a;

/// What the frame is drawn from besides the state.
#[derive(Debug, Clone)]
pub struct LaunchView {
    pub limits: Limits,
    pub version: VersionInfo,
    pub default_model_label: String,
    pub default_advisor_label: String,
    /// Account -> its resolved defaults (`Get-DefaultLabels` per root).
    pub labels_by_account: BTreeMap<String, DefaultLabels>,
    pub style: Style,
}

/// The launch screen over one state.
pub struct LaunchScreen<'a> {
    pub roster: &'a Roster,
    pub state: LaunchState,
    pub view: &'a LaunchView,
    pub prefs: &'a Prefs,
    pub now_ms: i64,
    cursor: Cursor,
    on_key: &'a mut OnKey<'a>,
}

impl std::fmt::Debug for LaunchScreen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchScreen")
            .field("state", &self.state)
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}

static NO_LABELS: std::sync::LazyLock<DefaultLabels> = std::sync::LazyLock::new(DefaultLabels::new);

impl<'a> LaunchScreen<'a> {
    pub fn new(
        roster: &'a Roster,
        state: LaunchState,
        view: &'a LaunchView,
        prefs: &'a Prefs,
        now_ms: i64,
        on_key: &'a mut OnKey<'a>,
    ) -> Self {
        let cursor = Cursor::at(i32::try_from(state.row).unwrap_or(0));
        Self {
            roster,
            state,
            view,
            prefs,
            now_ms,
            cursor,
            on_key,
        }
    }

    fn labels(&self) -> &'a DefaultLabels {
        self.view.labels_by_account.get(&self.state.account).unwrap_or(&NO_LABELS)
    }

    fn available(&self) -> Option<&'a [String]> {
        self.view
            .limits
            .get(&self.state.account)
            .and_then(|l| l.available_models.as_deref())
    }

    /// One walk of the stepper, and the tab switch the account row is.
    fn walk(&mut self, delta: i64, count: usize) {
        let leaving = self.state.account.clone();
        let ctx = OptionText::plain(self.available(), self.labels());
        for _ in 0..count {
            step_launch_value(self.roster, &mut self.state, delta, &ctx);
        }
        if self.roster.rows.get(self.state.row).is_some_and(|r| r.name == RowName::Account) && self.state.account != leaving {
            let to = std::mem::replace(&mut self.state.account, leaving);
            prefs::switch_account(&mut self.state, &to, self.prefs, self.roster, self.now_ms);
        }
        self.cursor.index = i32::try_from(self.state.row).unwrap_or(0);
    }
}

impl Screen for LaunchScreen<'_> {
    type Output = LaunchState;

    fn name(&self) -> &'static str {
        "launch"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    fn before(&mut self) {
        self.state.row = usize::try_from(self.cursor.index).unwrap_or(0);
        self.state.hover = self.cursor.hover;
        self.state.hover_row = self.cursor.hover_row;
        self.state.hover_value.clone_from(&self.cursor.hover_value);
        let available = self.available().map(<[String]>::to_vec);
        availability_snap(&mut self.state, available.as_deref());
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        let inp = LaunchFrameInput {
            state: &self.state,
            width: size.0,
            height: size.1,
            limits: &self.view.limits,
            version: &self.view.version,
            restored: None,
            default_model_label: &self.view.default_model_label,
            default_advisor_label: &self.view.default_advisor_label,
            default_labels: self.labels(),
            style: self.view.style,
        };
        launch_frame(self.roster, &inp)
    }

    fn rows(&self) -> Option<i32> {
        Some(i32::try_from(self.roster.rows.len()).unwrap_or(0))
    }

    fn has_left_right(&self) -> bool {
        true
    }

    fn left(&mut self) -> Flow<LaunchState> {
        self.walk(-1, 1);
        Flow::Continue
    }

    fn right(&mut self) -> Flow<LaunchState> {
        self.walk(1, 1);
        Flow::Continue
    }

    fn enter(&mut self) -> Option<Flow<LaunchState>> {
        Some(Flow::Done(Some(self.state.clone())))
    }

    /// A click on a row selects it; on an option cell it walks to that value through the same
    /// stepper the arrows use. Nothing here starts a session.
    fn click(&mut self, hit: &Hit) -> Flow<LaunchState> {
        let (row, value) = match hit {
            Hit::LaunchRow { row, .. } => (*row, None),
            Hit::Cell { row, value, .. } => (*row, Some(value.clone())),
            _ => return Flow::Continue,
        };
        self.cursor.index = i32::try_from(row).unwrap_or(0);
        self.state.row = row;
        let Some(value) = value else { return Flow::Continue };
        let Some(click_row) = self.roster.rows.get(row) else {
            return Flow::Continue;
        };
        let ctx = OptionText::plain(self.available(), self.labels());
        let values = visible_row_values(self.roster, click_row, &ctx);
        let cur = self.state.get(click_row.name).to_owned();
        let from_key = if is_hidden_row_value(self.roster, click_row, &cur, &ctx) {
            "default".to_owned()
        } else {
            cur.clone()
        };
        let pos = |k: &str| values.iter().position(|v| v == k);
        let (from, to) = (pos(&from_key), pos(&value));
        let (dir, steps) = match (from, to) {
            (Some(f), Some(t)) if f != t => (if t < f { -1 } else { 1 }, f.abs_diff(t)),
            (Some(f), Some(t)) if f == t && value == "default" && cur == "default" => (0, 1),
            _ => (1, 0),
        };
        self.walk(dir, steps);
        Flow::Continue
    }

    /// The caller's hook first, then ctrl+r (matched on the virtual key, so any layout works).
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        if (self.on_key)(key, term) {
            return true;
        }
        if key.is_ctrl(vk::R) {
            prefs::reset_tab(&mut self.state, self.roster);
            self.cursor.index = i32::try_from(self.state.row).unwrap_or(0);
            return true;
        }
        false
    }
}

#[cfg(test)]
#[expect(clippy::many_single_char_names, reason = "short fixture names")]
mod tests {
    use super::*;
    use crate::input::{Event, MouseEvent};
    use crate::launch::tests::four;
    use crate::ui::PreviewTerm;
    use crate::ui::screen_loop::run;

    fn view() -> LaunchView {
        LaunchView {
            limits: Limits::new(),
            version: VersionInfo::default(),
            default_model_label: "default".into(),
            default_advisor_label: "default".into(),
            labels_by_account: BTreeMap::new(),
            style: Style { color: false, ascii: true },
        }
    }

    fn drive(keys: &str) -> Option<LaunchState> {
        let r = four();
        let v = view();
        let p = Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let mut s = LaunchScreen::new(&r, r.new_state(), &v, &p, 0, &mut hook);
        let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys(keys).unwrap(), true);
        run(&mut s, &mut t).unwrap()
    }

    #[test]
    fn enter_alone_keeps_every_default() {
        let s = drive("Enter").unwrap();
        assert_eq!(
            (
                s.account.as_str(),
                s.remote.as_str(),
                s.action.as_str(),
                s.model.as_str(),
                s.mode.as_str()
            ),
            ("work", "on", "new", "default", "normal")
        );
    }

    #[test]
    fn escape_cancels() {
        assert!(drive("Escape").is_none());
    }

    #[test]
    fn right_on_account_switches_tab_and_d_aliases_right() {
        assert_eq!(drive("RightArrow,Enter").unwrap().account, "personal");
        assert_eq!(drive("d,d,Enter").unwrap().account, "low");
        assert_eq!(drive("LeftArrow,Enter").unwrap().account, "low", "wraps");
    }

    #[test]
    fn down_then_right_steps_the_model() {
        let s = drive("DownArrow,DownArrow,RightArrow,Enter").unwrap();
        assert_eq!(s.model, "fable");
        assert_eq!(s.row, 2);
    }

    #[test]
    fn cyrillic_d_steps_on_any_layout() {
        let s = drive("DownArrow,\u{0432},Enter").unwrap();
        assert_eq!(s.remote, "off");
    }

    #[test]
    fn ctrl_r_resets_the_tab() {
        let r = four();
        let v = view();
        let p = Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let mut st = r.new_state();
        st.effort = "max".into();
        st.restored = vec!["Effort".into()];
        let mut s = LaunchScreen::new(&r, st, &v, &p, 0, &mut hook);
        let ctrl_r = KeyPress {
            vk: vk::R,
            ctrl: true,
            ch: '\u{12}',
            ..KeyPress::default()
        };
        let mut t = PreviewTerm::new((100, 30), [ctrl_r, KeyPress::named(vk::ENTER)].into(), true);
        let out = run(&mut s, &mut t).unwrap().unwrap();
        assert_eq!(out.effort, "default");
        assert_eq!(out.restored, [] as [std::string::String; 0]);
    }

    #[test]
    fn caller_hook_sees_u_first() {
        let r = four();
        let v = view();
        let p = Prefs::default();
        let mut opened = 0;
        let mut hook = |k: &KeyPress, _: &mut dyn Term| {
            if crate::input::is_hotkey(k, 'u') {
                opened += 1;
                return true;
            }
            false
        };
        let mut s = LaunchScreen::new(&r, r.new_state(), &v, &p, 0, &mut hook);
        let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("u,Enter").unwrap(), true);
        assert!(run(&mut s, &mut t).unwrap().is_some());
        drop(s);
        assert_eq!(opened, 1);
    }

    struct Scripted {
        inner: PreviewTerm,
        q: std::collections::VecDeque<Event>,
    }

    impl Term for Scripted {
        fn size(&mut self) -> (usize, usize) {
            (100, 30)
        }
        fn paint(&mut self, lines: &[String]) {
            self.inner.paint(lines);
        }
        fn wait(&mut self, _s: (usize, usize)) -> Result<Event, crate::ui::UiStop> {
            self.q.pop_front().ok_or(crate::ui::UiStop::KeysExhausted)
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
        fn last_activation(&mut self) -> &mut Option<crate::ui::Activation> {
            self.inner.last_activation()
        }
        fn drain(&mut self) {}
        fn logger(&self) -> &crate::log::Logger {
            self.inner.logger()
        }
        fn read_line(&mut self, _prompt: &str) -> String {
            String::new()
        }
    }

    #[test]
    fn clicking_an_option_cell_selects_that_value_and_hover_bands_it() {
        let r = four();
        let v = LaunchView {
            style: Style {
                color: false,
                ascii: false,
            },
            ..view()
        };
        let p = Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let mut s = LaunchScreen::new(&r, r.new_state(), &v, &p, 0, &mut hook);
        // Effort row is y = 3 (box) + 1 (blank) + 3; 'max' sits after "   effort      ● [default] ○ low ...".
        let frame = s.draw((100, 30));
        let effort = frame.map.rows.iter().find(|h| h.name == "Effort").unwrap();
        let max = effort.cells.iter().find(|c| c.value == "max").unwrap();
        let mv = Event::Mouse(MouseEvent {
            x: max.start,
            y: effort.y,
            is_move: true,
            ..MouseEvent::default()
        });
        let press = Event::Mouse(MouseEvent {
            x: max.start,
            y: effort.y,
            left: true,
            buttons: 1,
            ..MouseEvent::default()
        });
        let mut t = Scripted {
            inner: PreviewTerm::new((100, 30), std::collections::VecDeque::default(), true),
            q: [mv, press, Event::Key(KeyPress::named(vk::ENTER))].into(),
        };
        let out = run(&mut s, &mut t).unwrap().unwrap();
        assert_eq!(out.effort, "max");
        assert_eq!(out.row, 3);
        let frames = t.inner.frames.unwrap();
        assert!(
            frames.get(1).unwrap().iter().any(|l| l.contains("max")),
            "the hovered frame still draws the row"
        );
    }
}
