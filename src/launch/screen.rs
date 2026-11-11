//! `Invoke-LaunchScreen`: what the launch screen alone does around the shared loop - stepping a
//! row over its visible values, the account row as a tab strip, click-to-value, ctrl+r, and the
//! caller's own key hook (`u` opens maintenance).

use std::collections::BTreeMap;

use super::frame::{LaunchFrameInput, launch_frame};
use super::{
    CONFIG_ROWS, LaunchState, OptionText, Roster, RowName, availability_snap, is_hidden_row_value, step_launch_value, visible_row_values,
};
use crate::config::RowsConfig;
use crate::frame::{Frame, Style};
use crate::input::{KeyPress, vk};
use crate::maintenance::{UpdateFound, UpdateWatch};
use crate::prefs::{self, Prefs};
use crate::types::{DefaultLabels, Limits, VersionInfo};
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};
use std::cell::RefCell;

/// The caller's hook: `true` consumed the key (the loop redraws).
pub type OnKey<'a> = dyn FnMut(&KeyPress, &mut dyn Term) -> bool + 'a;

/// Config `rows` and `hiddenValues` as the launch screen reads them; the seam that keeps tests off
/// the file.
pub trait RowsStore {
    /// The keys as the file holds them now; `rows` `None` = every row.
    fn load(&mut self) -> RowsConfig;
}

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
    pub roster: &'a mut Roster,
    pub state: LaunchState,
    /// A copy: the update check's answer changes it in place.
    pub view: LaunchView,
    pub prefs: &'a Prefs,
    pub now_ms: i64,
    cursor: Cursor,
    on_key: &'a mut OnKey<'a>,
    store: Option<&'a mut dyn RowsStore>,
    /// `rows` and `hiddenValues` as last read: a change on disk (maintenance `l`) is applied on return.
    disk: RowsConfig,
    /// The update check running behind the screen: its answer redraws the header and model row.
    update: Option<&'a RefCell<UpdateWatch>>,
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

fn labels_of<'v>(view: &'v LaunchView, account: &str) -> &'v DefaultLabels {
    view.labels_by_account.get(account).unwrap_or(&NO_LABELS)
}

fn available_of<'v>(view: &'v LaunchView, account: &str) -> Option<&'v [String]> {
    view.limits.get(account).and_then(|l| l.available_models.as_deref())
}

impl LaunchView {
    /// The update check's answer: the header's versions and, after an install, the `default (X)`
    /// labels read against the new catalog.
    pub(crate) fn apply(&mut self, found: &UpdateFound) {
        self.version.clone_from(&found.version);
        if let Some(d) = &found.defaults {
            self.default_model_label.clone_from(&d.model);
            self.default_advisor_label.clone_from(&d.advisor);
            self.labels_by_account.clone_from(&d.by_account);
        }
    }
}

impl<'a> LaunchScreen<'a> {
    pub fn new(
        roster: &'a mut Roster,
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
            view: view.clone(),
            prefs,
            now_ms,
            cursor,
            on_key,
            store: None,
            disk: RowsConfig::default(),
            update: None,
        }
    }

    /// `rows` read from `store` now and after the caller's hook: a change shows on return.
    #[must_use]
    pub fn with_rows(mut self, store: &'a mut dyn RowsStore) -> Self {
        self.disk = store.load();
        self.store = Some(store);
        self
    }

    /// The update check behind the screen; an answer already in shows in the first frame.
    #[must_use]
    pub(crate) fn with_update(mut self, update: &'a RefCell<UpdateWatch>) -> Self {
        self.update = Some(update);
        self.take_update();
        self.apply_update();
        self
    }

    /// Applies the check's answer once it is in; `true` when the screen must redraw. Only the
    /// labels move: the selection stays, and `before` snaps a model gone unavailable as always.
    fn take_update(&mut self) -> bool {
        let news = self.update.is_some_and(|u| u.borrow_mut().news().is_some());
        if news {
            self.apply_update();
        }
        news
    }

    /// The answer a screen was handed: header, model-row labels, `default (X)` labels.
    fn apply_update(&mut self) {
        let Some(update) = self.update else { return };
        let watch = update.borrow();
        let Some(found) = watch.applied() else { return };
        if let Some(labels) = &found.labels {
            self.roster.set_model_labels(labels);
        }
        self.view.apply(found);
    }

    fn labels(&self) -> &DefaultLabels {
        labels_of(&self.view, &self.state.account)
    }

    fn available(&self) -> Option<&[String]> {
        available_of(&self.view, &self.state.account)
    }

    /// One walk of the stepper, and the tab switch the account row is.
    fn walk(&mut self, delta: i64, count: usize) {
        let leaving = self.state.account.clone();
        let ctx = OptionText::plain(available_of(&self.view, &leaving), labels_of(&self.view, &leaving));
        for _ in 0..count {
            step_launch_value(self.roster, &mut self.state, delta, &ctx);
        }
        if self.roster.rows.get(self.state.row).is_some_and(|r| r.name == RowName::Account) && self.state.account != leaving {
            let to = std::mem::replace(&mut self.state.account, leaving);
            prefs::switch_account(&mut self.state, &to, self.prefs, self.roster, self.now_ms);
        }
        self.cursor.index = i32::try_from(self.state.row).unwrap_or(0);
    }

    /// Shows `rows` at once: a row that left the screen launches with its default or its
    /// `hiddenValues` pin, as a row hidden at start does; the cursor stays on its row, or the next
    /// shown one.
    fn apply(&mut self, rows: &[RowName], keep: Option<RowName>) {
        self.roster.apply_rows(rows);
        let fresh = self.roster.new_state();
        for gone in CONFIG_ROWS
            .iter()
            .filter(|r| self.roster.row(**r).is_none() && **r != RowName::Account)
        {
            self.state.set(*gone, fresh.get(*gone));
            self.state.restored.retain(|r| !r.eq_ignore_ascii_case(gone.as_str()));
        }
        let last = self.roster.rows.len().saturating_sub(1);
        let at = keep.and_then(|k| self.roster.rows.iter().position(|r| r.name == k));
        self.state.row = at.unwrap_or(self.state.row).min(last);
        self.cursor.index = i32::try_from(self.state.row).unwrap_or(0);
    }

    /// After the caller's hook (maintenance `l` may have saved `rows`): a changed file shows now.
    fn reload_rows(&mut self) {
        let Some(store) = self.store.as_deref_mut() else { return };
        let now = store.load();
        if now.rows != self.disk.rows || now.hidden_values != self.disk.hidden_values {
            let rows = now.rows.clone().unwrap_or_else(|| CONFIG_ROWS.to_vec());
            self.roster.set_hidden_values(&now.hidden_values);
            self.disk = now;
            let keep = self.roster.rows.get(self.state.row).map(|r| r.name);
            self.apply(&rows, keep);
        }
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

    fn idle(&mut self) -> bool {
        self.take_update()
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

    /// The caller's hook first (then a `rows` maintenance `l` saved shows), then ctrl+r (matched
    /// on the virtual key, so any layout works).
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        if (self.on_key)(key, term) {
            self.reload_rows();
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
        let mut r = four();
        let v = view();
        let p = Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let st = r.new_state();
        let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
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
        assert_eq!(s.model, "haiku", "weakest first");
        assert_eq!(s.row, 2);
    }

    #[test]
    fn cyrillic_d_steps_on_any_layout() {
        let s = drive("DownArrow,\u{0432},Enter").unwrap();
        assert_eq!(s.remote, "off");
    }

    #[test]
    fn ctrl_r_resets_the_tab() {
        let mut r = four();
        let v = view();
        let p = Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let mut st = r.new_state();
        st.effort = "max".into();
        st.restored = vec!["Effort".into()];
        let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
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
        let mut r = four();
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
        let st = r.new_state();
        let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
        let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("u,Enter").unwrap(), true);
        assert!(run(&mut s, &mut t).unwrap().is_some());
        drop(s);
        assert_eq!(opened, 1);
    }

    #[test]
    fn clicking_an_option_cell_selects_that_value_and_hover_bands_it() {
        let mut r = four();
        let v = LaunchView {
            style: Style {
                color: false,
                ascii: false,
            },
            ..view()
        };
        let p = Prefs::default();
        let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
        let st = r.new_state();
        let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
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
        let mut t = crate::ui::script_term::ScriptTerm::new((100, 30), [mv, press, Event::Key(KeyPress::named(vk::ENTER))]);
        let out = run(&mut s, &mut t).unwrap().unwrap();
        assert_eq!(out.effort, "max");
        assert_eq!(out.row, 3);
        let frames = t.inner.frames.unwrap();
        assert!(
            frames.get(1).unwrap().iter().any(|l| l.contains("max")),
            "the hovered frame still draws the row"
        );
    }

    /// Config `rows` in memory. `pending` lands on the second load: what maintenance `l` saved.
    #[derive(Default)]
    struct FakeStore {
        disk: RowsConfig,
        pending: Option<RowsConfig>,
        loads: usize,
    }

    impl RowsStore for FakeStore {
        fn load(&mut self) -> RowsConfig {
            self.loads += 1;
            if self.loads > 1
                && let Some(p) = self.pending.take()
            {
                self.disk = p;
            }
            self.disk.clone()
        }
    }

    /// Runs the screen over `r` with `store`; the hook takes `u` as maintenance would.
    fn edit(r: &mut Roster, st: LaunchState, store: &mut FakeStore, keys: &str) -> (Option<LaunchState>, Vec<Vec<String>>) {
        let v = view();
        let p = Prefs::default();
        let mut hook = |k: &KeyPress, _: &mut dyn Term| crate::input::is_hotkey(k, 'u');
        let mut s = LaunchScreen::new(r, st, &v, &p, 0, &mut hook).with_rows(store);
        let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys(keys).unwrap(), true);
        let out = run(&mut s, &mut t).unwrap();
        (out, t.frames.unwrap())
    }

    fn names(r: &Roster) -> Vec<RowName> {
        r.rows.iter().map(|x| x.name).collect()
    }

    const ALL: [RowName; 7] = crate::launch::CONFIG_ROWS;

    #[test]
    fn row_keys_change_no_rows_on_the_launch_screen() {
        let mut r = four();
        let mut store = FakeStore::default();
        let st = r.new_state();
        // One key per run: a move and its reverse in one run would cancel out.
        for keys in [
            "DownArrow,DownArrow,Ctrl+S,Enter",
            "DownArrow,DownArrow,Ctrl+W,Enter",
            "DownArrow,DownArrow,Ctrl+X,Enter",
        ] {
            let st = r.new_state();
            let (out, _) = edit(&mut r, st, &mut store, keys);
            assert_eq!(out.unwrap().row, 2, "{keys}: the cursor stays");
            assert_eq!(names(&r), ALL, "{keys}");
        }
        let (_, frames) = edit(&mut r, st, &mut store, "o,Enter");
        assert!(
            !frames.iter().any(|f| f.iter().any(|l| l.contains("launch rows"))),
            "no editor here: maintenance l has it"
        );
    }

    #[test]
    fn rows_saved_in_maintenance_apply_on_return() {
        let mut r = four();
        let mut store = FakeStore {
            pending: Some(RowsConfig {
                rows: Some(vec![RowName::Account, RowName::Mode]),
                ..RowsConfig::default()
            }),
            ..FakeStore::default()
        };
        let st = r.new_state();
        let (out, _) = edit(&mut r, st, &mut store, "u,Enter");
        assert!(out.is_some());
        assert_eq!(names(&r), [RowName::Account, RowName::Mode]);
    }

    #[test]
    fn a_row_hidden_mid_run_never_takes_a_value_from_another_tabs_stash() {
        // work: effort -> low, advisor -> fable; tab to personal; maintenance hides both (effort
        // pinned high); tab back to work: the stash must not bring low or fable back.
        let keys =
            "DownArrow,DownArrow,DownArrow,RightArrow,DownArrow,RightArrow,UpArrow,UpArrow,UpArrow,UpArrow,RightArrow,u,LeftArrow,Enter";
        let mut r = four();
        let mut store = FakeStore {
            pending: Some(RowsConfig {
                rows: Some(vec![RowName::Account, RowName::Model]),
                hidden_values: BTreeMap::from([(RowName::Effort, "high".to_owned())]),
                ..RowsConfig::default()
            }),
            ..FakeStore::default()
        };
        let st = r.new_state();
        let out = edit(&mut r, st, &mut store, keys).0.unwrap();
        assert_eq!(out.account, "work", "back on the first tab");
        assert_eq!((out.effort.as_str(), out.advisor.as_str()), ("high", "default"));
        assert!(!out.is_restored(RowName::Effort) && !out.is_restored(RowName::Advisor));
        assert_eq!(
            super::super::launch_args(&r, &out, None, false, None, "", "").join(" "),
            "--effort high"
        );
    }

    #[test]
    fn hidden_values_saved_in_maintenance_apply_on_return() {
        let mut r = four();
        let mut store = FakeStore {
            pending: Some(RowsConfig {
                rows: Some(vec![RowName::Account, RowName::Mode]),
                hidden_values: BTreeMap::from([(RowName::Effort, "high".to_owned())]),
                ..RowsConfig::default()
            }),
            ..FakeStore::default()
        };
        let st = r.new_state();
        let out = edit(&mut r, st, &mut store, "u,Enter").0.unwrap();
        assert_eq!(out.effort, "high", "the hidden effort launches with its pin");
        assert_eq!(out.model, "default", "an unpinned hidden row keeps its default");
    }
}

#[cfg(test)]
mod update_tests;
