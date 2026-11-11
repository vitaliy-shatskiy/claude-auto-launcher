//! The launch-rows editor behind the maintenance screen's `l`: which launch rows show and in what
//! order, saved as config `rows`, and what a hidden row launches with, saved as `hiddenValues`.
//! w/s cursor, ctrl+w/s (alt+up/down) move, ctrl+x or space show/hide, a/d (left/right) a hidden
//! row's value, enter save, esc cancel. A hidden row stays in place while editing, dimmed; `rows`
//! holds only the shown rows, so on reopen the hidden ones follow them in the default order. With
//! config `remote` off the remote entry keeps its place and shown flag but takes no key.
//! Mouse: a click selects a row, a second click on the value of a hidden row that click selected
//! steps it as d does; the footer buttons stand for ctrl+w/s, space, enter and esc.

use std::collections::BTreeMap;

use crate::config::RowsConfig;
use crate::frame::{
    CellSpan, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RowHit, RowMap, Style, complete_frame, footer_hover, frame_width,
    new_hint_footer, too_small_frame,
};
use crate::input::{KeyPress, vk};
use crate::launch::{CONFIG_ROWS, LaunchState, Roster, RowName, step_option};
use crate::layout::{display_width, new_box};
use crate::theme::{DIM_CLOSE, DIM_OPEN, add_hover_span, glyphs};
use crate::ui::screen_loop::{Cursor, Flow, Screen};
use crate::ui::{Hit, Term};

const REMOTE_OFF: &str = "remote is off: set \"remote\": true in claude-auto.json";

/// What the move buttons send: the chars ctrl+w and ctrl+s type, so a click needs no modifier.
const CTRL_W: char = '\u{17}';
const CTRL_S: char = '\u{13}';
/// The click cell of a hidden row's value.
const VALUE_CELL: &str = "value";

/// What enter saves: `rows`, then `hiddenValues`.
pub(crate) type RowsChoice = (Vec<RowName>, BTreeMap<RowName, String>);

/// Every config row with its shown flag, in the order being edited.
pub(crate) struct RowsEditor {
    pub(crate) entries: Vec<(RowName, bool)>,
    /// A hidden row's launch value; absent = the row's default.
    values: BTreeMap<RowName, String>,
    /// The rows' values and labels.
    catalog: Roster,
    /// Config `remote`: off leaves the remote entry inert.
    remote: bool,
    cursor: Cursor,
    style: Style,
    notice: &'static str,
    /// The row the last click selected; a key or the wheel clears it (D1).
    clicked: Option<usize>,
}

impl std::fmt::Debug for RowsEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowsEditor").field("entries", &self.entries).finish_non_exhaustive()
    }
}

impl RowsEditor {
    /// The shown rows first in their order, the hidden ones after them in the default order.
    pub(crate) fn new(current: &RowsConfig, style: Style) -> Self {
        let shown = current.rows.as_deref().unwrap_or(&CONFIG_ROWS);
        let mut entries: Vec<(RowName, bool)> = shown.iter().map(|r| (*r, true)).collect();
        entries.extend(CONFIG_ROWS.iter().filter(|r| !shown.contains(r)).map(|r| (*r, false)));
        Self {
            entries,
            values: current.hidden_values.clone(),
            catalog: Roster::catalog(current.model_order.as_deref()),
            remote: current.remote,
            cursor: Cursor::at(0),
            style,
            notice: "",
            clicked: None,
        }
    }

    /// The `rows` value to save: the shown rows in order.
    pub(crate) fn rows(&self) -> Vec<RowName> {
        self.entries.iter().filter(|(_, on)| *on).map(|(r, _)| *r).collect()
    }

    /// The `hiddenValues` to save: hidden rows whose value is not their default.
    pub(crate) fn hidden_values(&self) -> BTreeMap<RowName, String> {
        self.entries
            .iter()
            .filter(|(r, on)| !on && self.value(*r) != default_of(*r))
            .map(|(r, _)| (*r, self.value(*r)))
            .collect()
    }

    fn value(&self, row: RowName) -> String {
        self.values.get(&row).cloned().unwrap_or_else(|| default_of(row))
    }

    /// `default` reads as what it means: claude picks.
    fn value_label(&self, row: RowName) -> String {
        let v = self.value(row);
        if v == "default" {
            return "claude default".into();
        }
        self.catalog.def(row).and_then(|d| d.labels.get(&v)).cloned().unwrap_or(v)
    }

    /// The remote entry while config `remote` is off: drawn, kept in place, never changed.
    fn inert(&self, row: RowName) -> bool {
        row == RowName::Remote && !self.remote
    }

    fn index(&self) -> usize {
        usize::try_from(self.cursor.index).unwrap_or(0)
    }

    fn shift(&mut self, delta: isize) {
        let i = self.index();
        let Some(j) = i.checked_add_signed(delta).filter(|j| *j < self.entries.len()) else {
            return;
        };
        self.entries.swap(i, j);
        self.cursor.index = i32::try_from(j).unwrap_or(0);
    }

    fn toggle(&mut self) {
        let i = self.index();
        let remote = self.remote;
        if let Some((row, on)) = self.entries.get_mut(i) {
            if *row == RowName::Account {
                self.notice = "account is always shown";
            } else if *row == RowName::Remote && !remote {
                self.notice = REMOTE_OFF;
            } else {
                *on = !*on;
            }
        }
    }

    /// a/d on a hidden row: its launch value, over the row's own values.
    fn step(&mut self, delta: i64) {
        let Some(&(row, on)) = self.entries.get(self.index()) else { return };
        if self.inert(row) {
            self.notice = REMOTE_OFF;
            return;
        }
        if on {
            self.notice = "a/d sets a hidden row's value; pick a shown row's on the launch screen";
            return;
        }
        let Some(def) = self.catalog.def(row) else { return };
        let next = step_option(&def.values, &self.value(row), delta);
        self.values.insert(row, next);
    }
}

/// The value a row launches with when nothing picks one (`normal` for mode, `on` for remote).
fn default_of(row: RowName) -> String {
    LaunchState::fresh("").get(row).to_owned()
}

impl Screen for RowsEditor {
    type Output = RowsChoice;

    fn name(&self) -> &'static str {
        "rows"
    }

    fn cursor(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    fn draw(&mut self, size: (usize, usize)) -> Frame {
        let (width, height) = size;
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            return Frame {
                lines: too_small_frame(width, height, self.style.ascii),
                map: RowMap::default(),
            };
        }
        let g = glyphs(self.style.ascii);
        let fw = frame_width(width);
        let box_w = fw.min(100);
        let mut hits = Vec::new();
        let mut body: Vec<String> = Vec::new();
        for (i, (row, on)) in self.entries.iter().enumerate() {
            let mark = if i == self.index() { g.cursor } else { ' ' };
            let name = row.as_str().to_lowercase();
            let glyph = if *on { g.on } else { g.off };
            let lead = format!(" {mark} {glyph} {name:<12}");
            let mut cells = Vec::new();
            body.push(if self.inert(*row) {
                format!(" {mark} {DIM_OPEN}{glyph} {name:<12}off in config (remote: false){DIM_CLOSE}")
            } else if *on {
                format!("{lead}shown")
            } else {
                let value = format!("hidden -> {}", self.value_label(*row));
                // 1 = the box's left border; the cell never reaches past the right one.
                let start = 1 + display_width(&lead);
                let end = (start + display_width(&value)).min(box_w.saturating_sub(1)).saturating_sub(1);
                cells.push(CellSpan {
                    start: i32::try_from(start).unwrap_or(i32::MAX),
                    end: i32::try_from(end).unwrap_or(0),
                    value: VALUE_CELL.into(),
                });
                let hovered = i32::try_from(i).is_ok_and(|ii| ii == self.cursor.hover_row) && self.cursor.hover_value == VALUE_CELL;
                let value = if hovered { add_hover_span(&value) } else { value };
                format!(" {mark} {DIM_OPEN}{glyph} {name:<12}{value}{DIM_CLOSE}")
            });
            hits.push(RowHit {
                index: i,
                name: row.as_str().into(),
                // 1 = the box's top border.
                y: i32::try_from(1 + i).unwrap_or(0),
                cells,
            });
        }
        body.push(String::new());
        body.push("  a/d or a click on its value: what a hidden row launches with".into());
        body.push(format!(
            "  {}",
            if self.notice.is_empty() {
                "saved to the config; the launch screen shows it on return"
            } else {
                self.notice
            }
        ));
        let footer = new_hint_footer(
            &[
                // Five buttons fill 76 columns; a/d lives in the body, as a sixth hint wraps at 78.
                Hint::ch("^w", "up", CTRL_W),
                Hint::ch("^s", "down", CTRL_S),
                Hint::ch("^x", "show/hide", ' '),
                Hint::key("enter", "save", "Enter"),
                Hint::key("esc", "cancel", "Escape"),
            ],
            &g,
            fw,
            !self.style.color,
        );
        let light = FooterLight {
            hover: footer_hover(&footer, self.cursor.hover),
            selected: Vec::new(),
        };
        let lines = new_box(&body, box_w, "launch rows", &g, self.style.ascii);
        let mut map = RowMap {
            rows: hits,
            ..RowMap::default()
        };
        let painter = |l: &str, e: bool| crate::list::add_picker_color(l, e, &g);
        let lines = complete_frame(&lines, &footer, fw, self.style, &painter, &light, &mut map);
        Frame { lines, map }
    }

    fn rows(&self) -> Option<i32> {
        Some(i32::try_from(self.entries.len()).unwrap_or(0))
    }

    fn enter(&mut self) -> Option<Flow<RowsChoice>> {
        Some(Flow::Done(Some((self.rows(), self.hidden_values()))))
    }

    fn has_left_right(&self) -> bool {
        true
    }

    fn left(&mut self) -> Flow<RowsChoice> {
        self.step(-1);
        Flow::Continue
    }

    fn right(&mut self) -> Flow<RowsChoice> {
        self.step(1);
        Flow::Continue
    }

    /// The wheel moves the cursor as keys do: it never arms a click.
    fn wheel(&mut self, _delta: i32) -> bool {
        self.clicked = None;
        false
    }

    /// D1: a click selects a row; a second click on the value of the hidden row the previous
    /// click selected steps it as d does. Only a hidden, live row has a value cell.
    fn click(&mut self, hit: &Hit) -> Flow<RowsChoice> {
        let (row, on_value) = match hit {
            Hit::LaunchRow { row, .. } => (*row, false),
            Hit::Cell { row, value, .. } => (*row, value == VALUE_CELL),
            Hit::None | Hit::Footer { .. } | Hit::Action { .. } | Hit::ListRow(_) => return Flow::Continue,
        };
        self.notice = "";
        let armed = self.clicked == Some(row);
        self.clicked = Some(row);
        self.cursor.index = i32::try_from(row).unwrap_or(0);
        if armed && on_value {
            self.step(1);
        }
        Flow::Continue
    }

    /// ctrl+w/s move the row, matched on the virtual key (any layout); alt+up/down too, as ctrl+s
    /// is XOFF on some terminals. ctrl+x or space shows or hides it. Ahead of the loop's own w/s.
    /// Never shift+w/s: a split mouse report arrives as shift+letter.
    /// A footer button arrives as its char alone (`CTRL_W`, `CTRL_S`, space).
    fn on_key(&mut self, key: &KeyPress, _term: &mut dyn Term) -> bool {
        self.notice = "";
        self.clicked = None;
        let up = key.is_ctrl(vk::W) || key.ch == CTRL_W || (key.alt && key.is(vk::UP));
        let down = key.is_ctrl(vk::S) || key.ch == CTRL_S || (key.alt && key.is(vk::DOWN));
        if up || down {
            self.shift(if up { -1 } else { 1 });
            return true;
        }
        if key.is_ctrl(vk::X) || key.ch == ' ' || key.is(vk::SPACE) {
            self.toggle();
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Event, MouseEvent};
    use crate::ui::{PreviewTerm, parse_preview_keys};

    fn config(rows: Option<&[RowName]>) -> RowsConfig {
        RowsConfig {
            rows: rows.map(<[RowName]>::to_vec),
            remote: true,
            ..RowsConfig::default()
        }
    }

    fn edit_config(current: &RowsConfig, keys: Vec<KeyPress>) -> Option<RowsChoice> {
        let mut e = RowsEditor::new(current, Style { color: false, ascii: true });
        let mut t = PreviewTerm::new((100, 30), keys.into(), true);
        crate::ui::screen_loop::run(&mut e, &mut t).unwrap()
    }

    fn edit(current: Option<&[RowName]>, keys: Vec<KeyPress>) -> Option<Vec<RowName>> {
        edit_config(&config(current), keys).map(|(rows, _)| rows)
    }

    fn pins(p: &[(RowName, &str)]) -> BTreeMap<RowName, String> {
        p.iter().map(|(r, v)| (*r, (*v).to_owned())).collect()
    }

    #[test]
    fn remote_off_is_dimmed_inert_and_keeps_its_place_on_save() {
        let off = RowsConfig {
            remote: false,
            ..config(None)
        };
        let mut e = RowsEditor::new(&off, Style { color: true, ascii: false });
        let f = e.draw((100, 30));
        let line = f.lines.iter().find(|l| l.contains("remote")).unwrap();
        assert!(line.contains("off in config (remote: false)"), "{line}");
        assert!(line.contains(crate::theme::DIM), "{line:?}");
        let (rows, hidden) = edit_config(&off, keys("s,Ctrl+X,Spacebar,d,a,RightArrow,Enter")).unwrap();
        assert_eq!(rows, CONFIG_ROWS, "remote still shown, still second");
        assert!(hidden.is_empty());
        e.cursor.index = 1;
        e.toggle();
        assert_eq!(e.notice, REMOTE_OFF);
        let moved = edit_config(&off, keys("s,Ctrl+S,Enter")).unwrap().0;
        assert_eq!(moved.get(2), Some(&RowName::Remote), "moves keep working: the place is the user's");
    }

    #[test]
    fn remote_off_and_hidden_keeps_its_saved_value() {
        let off = RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Model]),
            hidden_values: pins(&[(RowName::Remote, "off")]),
            remote: false,
            ..RowsConfig::default()
        };
        let (rows, hidden) = edit_config(&off, keys("s,s,d,Enter")).unwrap();
        assert_eq!(rows, [RowName::Account, RowName::Model], "a hidden remote stays out of rows");
        assert_eq!(hidden, pins(&[(RowName::Remote, "off")]), "d did nothing to it");
    }

    #[test]
    fn a_d_cycle_a_hidden_rows_value_and_save_writes_it() {
        let cur = config(Some(&[RowName::Account, RowName::Mode]));
        // Hidden after account, mode: remote, model, effort, ...
        let (_, hidden) = edit_config(&cur, keys("s,s,s,d,d,d,d,s,a,Enter")).unwrap();
        assert_eq!(hidden, pins(&[(RowName::Model, "fable"), (RowName::Effort, "ultracode")]));
        let (_, hidden) = edit_config(&cur, keys("s,s,s,RightArrow,LeftArrow,LeftArrow,Enter")).unwrap();
        assert_eq!(hidden, pins(&[(RowName::Model, "fable")]), "arrows too, and a wraps");
    }

    #[test]
    fn the_default_value_writes_nothing_and_a_shown_row_drops_its_entry() {
        let cur = RowsConfig {
            hidden_values: pins(&[(RowName::Model, "fable"), (RowName::Mode, "safe")]),
            ..config(Some(&[RowName::Account]))
        };
        // Model (index 2): d wraps fable -> default. Mode (last): shown again.
        let (rows, hidden) = edit_config(&cur, keys("s,s,d,s,s,s,s,Spacebar,Enter")).unwrap();
        assert_eq!(rows, [RowName::Account, RowName::Mode]);
        assert!(hidden.is_empty(), "{hidden:?}");
    }

    #[test]
    fn a_hidden_remote_cycles_only_on_and_off() {
        let cur = config(Some(&[RowName::Account]));
        for (keys_, want) in [("s,d,Enter", Some("off")), ("s,d,d,Enter", None), ("s,a,Enter", Some("off"))] {
            let (_, hidden) = edit_config(&cur, keys(keys_)).unwrap();
            assert_eq!(hidden.get(&RowName::Remote).map(String::as_str), want, "{keys_}");
        }
    }

    #[test]
    fn a_d_on_a_shown_row_changes_nothing() {
        let cur = config(Some(&[RowName::Account, RowName::Effort]));
        let (rows, hidden) = edit_config(&cur, keys("s,d,a,Enter")).unwrap();
        assert_eq!(rows, [RowName::Account, RowName::Effort]);
        assert!(hidden.is_empty());
    }

    #[test]
    fn a_hidden_row_reads_its_value_by_label() {
        let cur = RowsConfig {
            hidden_values: pins(&[(RowName::Model, "fable")]),
            ..config(Some(&[RowName::Account]))
        };
        let mut e = RowsEditor::new(&cur, Style { color: false, ascii: true });
        let f = e.draw((100, 30));
        let line = |n: &str| f.lines.iter().find(|l| l.contains(n)).cloned().unwrap();
        assert!(line("model").contains("hidden -> Fable 5.2"), "{}", line("model"));
        assert!(line("effort").contains("hidden -> claude default"), "{}", line("effort"));
        assert!(line("mode ").contains("hidden -> normal"), "{}", line("mode "));
    }

    fn keys(s: &str) -> Vec<KeyPress> {
        parse_preview_keys(s).unwrap().into()
    }

    const FIRST_SIX: [RowName; 6] = [
        RowName::Account,
        RowName::Remote,
        RowName::Model,
        RowName::Effort,
        RowName::Advisor,
        RowName::Permission,
    ];

    #[test]
    fn ctrl_x_hides_a_row_and_enter_saves_the_shown_ones() {
        assert_eq!(edit(None, keys("s,s,s,s,s,s,Ctrl+X,Enter")).unwrap(), FIRST_SIX);
        assert_eq!(edit(None, keys("s,s,s,s,s,s,Spacebar,Enter")).unwrap(), FIRST_SIX, "space too");
    }

    #[test]
    fn ctrl_s_moves_a_row_down_and_ctrl_w_up_and_hidden_rows_follow_the_shown_ones() {
        let out = edit(Some(&[RowName::Account, RowName::Model, RowName::Effort]), keys("s,Ctrl+S,Enter")).unwrap();
        assert_eq!(out, [RowName::Account, RowName::Effort, RowName::Model]);
        let out = edit(Some(&[RowName::Account, RowName::Model, RowName::Effort]), keys("s,s,Ctrl+W,Enter")).unwrap();
        assert_eq!(out, [RowName::Account, RowName::Effort, RowName::Model]);
        let e = RowsEditor::new(&config(Some(&[RowName::Mode])), Style { color: false, ascii: true });
        assert_eq!(e.entries.first(), Some(&(RowName::Mode, true)));
        assert!(e.entries.iter().skip(1).all(|(_, on)| !on), "the rest listed hidden");
    }

    #[test]
    fn alt_arrows_move_and_shift_letters_do_not() {
        let alt = |code| KeyPress {
            alt: true,
            ..KeyPress::named(code)
        };
        let shifted = |c| KeyPress {
            shift: true,
            ..KeyPress::char(c)
        };
        let mut k = keys("s");
        k.extend([alt(vk::DOWN), alt(vk::DOWN), alt(vk::UP), shifted('S')]);
        k.extend(keys("Enter"));
        let out = edit(None, k).unwrap();
        assert_eq!(out.get(2), Some(&RowName::Remote), "shift+s left remote where alt put it");
        assert_eq!(
            out.get(..4),
            Some(&[RowName::Account, RowName::Model, RowName::Remote, RowName::Effort][..])
        );
    }

    #[test]
    fn account_cannot_be_hidden_and_escape_cancels() {
        assert_eq!(edit(None, keys("Ctrl+X,Spacebar,Enter")).unwrap(), CONFIG_ROWS, "account stays");
        assert_eq!(edit(None, keys("s,Ctrl+X,Escape")), None);
    }

    #[test]
    fn a_hidden_row_stays_in_place_dimmed_and_the_footer_names_the_keys() {
        let mut e = RowsEditor::new(&config(None), Style { color: true, ascii: false });
        e.cursor.index = 2;
        e.toggle();
        let f = e.draw((100, 30));
        let model = f.lines.iter().position(|l| l.contains("model")).unwrap();
        let effort = f.lines.iter().position(|l| l.contains("effort")).unwrap();
        assert!(model < effort, "kept in place");
        assert!(f.lines[model].contains(crate::theme::DIM), "{:?}", f.lines[model]);
        assert!(!f.lines[effort].contains(&format!("{}effort", crate::theme::DIM)));
        assert!(f.lines.iter().any(|l| l.contains("a/d")), "the body names a/d");
        for size in [(100, 30), (78, 24)] {
            let f = e.draw(size);
            assert_eq!(f.map.footer_lines, 1, "one footer line at {size:?}");
            let foot = f.lines.last().unwrap();
            for h in ["^w", "up", "^s", "down", "^x", "show/hide", "save", "cancel"] {
                assert!(foot.contains(h), "{h} at {size:?}: {foot}");
            }
            assert_eq!(f.map.footer.len(), 5, "every hint a button at {size:?}");
        }
    }

    /// Scripted events, mouse included.
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

    fn press((x, y): (i32, i32)) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            left: true,
            buttons: 1,
            ..MouseEvent::default()
        })
    }

    fn key(name: &str) -> Event {
        Event::Key(keys(name).remove(0))
    }

    fn run_events(cur: &RowsConfig, events: Vec<Event>) -> Option<RowsChoice> {
        let mut e = RowsEditor::new(cur, Style { color: false, ascii: true });
        let mut t = Scripted {
            inner: PreviewTerm::new((100, 30), std::collections::VecDeque::default(), true),
            q: events.into(),
        };
        crate::ui::screen_loop::run(&mut e, &mut t).unwrap()
    }

    /// A point on a row's line, and on its value cell if it has one.
    fn row_at(cur: &RowsConfig, row: RowName) -> ((i32, i32), Option<(i32, i32)>) {
        let mut e = RowsEditor::new(cur, Style { color: false, ascii: true });
        let f = e.draw((100, 30));
        let hit = f.map.rows.iter().find(|h| h.name == row.as_str()).unwrap();
        ((2, hit.y), hit.cells.first().map(|c| (c.start, hit.y)))
    }

    /// The footer button that stands for (key, char).
    fn button(cur: &RowsConfig, id: (&str, Option<char>)) -> (i32, i32) {
        let mut e = RowsEditor::new(cur, Style { color: false, ascii: true });
        let f = e.draw((100, 30));
        let s = f.map.footer.iter().find(|s| (s.key.as_str(), s.ch) == id).unwrap();
        (s.start, f.map.footer_y.unwrap() + i32::try_from(s.line).unwrap())
    }

    #[test]
    fn a_click_selects_a_row_and_a_second_click_on_its_hidden_value_steps_it() {
        let cur = config(Some(&[RowName::Account]));
        let cell = row_at(&cur, RowName::Model).1.expect("a hidden row has a value cell");
        let once = run_events(&cur, vec![press(cell), key("Enter")]).unwrap();
        assert!(once.1.is_empty(), "the first click only selects: {:?}", once.1);
        let twice = run_events(&cur, vec![press(cell), press(cell), key("Enter")]).unwrap();
        let by_key = edit_config(&cur, keys("s,s,d,Enter")).unwrap();
        assert!(!twice.1.is_empty());
        assert_eq!(twice.1, by_key.1, "the second click is d");
    }

    #[test]
    fn a_keyboard_moved_cursor_does_not_arm_a_click() {
        let cur = config(Some(&[RowName::Account]));
        let (line, cell) = row_at(&cur, RowName::Model);
        let cell = cell.unwrap();
        let out = run_events(&cur, vec![key("s"), key("s"), press(cell), key("Enter")]).unwrap();
        assert!(out.1.is_empty(), "keys put the cursor there, the click only selects: {:?}", out.1);
        let out = run_events(&cur, vec![press(line), press(cell), key("Enter")]).unwrap();
        assert!(!out.1.is_empty(), "a click on the row's line arms its value");
        let out = run_events(&cur, vec![press(cell), key("w"), key("s"), press(cell), key("Enter")]).unwrap();
        assert!(out.1.is_empty(), "a key between the clicks disarms: {:?}", out.1);
    }

    #[test]
    fn a_shown_row_and_the_remote_off_entry_have_no_value_cell_and_clicks_change_nothing() {
        let off = RowsConfig {
            rows: Some(vec![RowName::Account, RowName::Model]),
            remote: false,
            ..RowsConfig::default()
        };
        let (remote, cell) = row_at(&off, RowName::Remote);
        assert_eq!(cell, None, "remote off: inert");
        let (model, cell) = row_at(&off, RowName::Model);
        assert_eq!(cell, None, "a shown row's value is picked on the launch screen");
        let far = |(_, y): (i32, i32)| (40, y);
        let events = vec![
            press(remote),
            press(remote),
            press(far(remote)),
            press(model),
            press(far(model)),
            key("Enter"),
        ];
        let out = run_events(&off, events).unwrap();
        assert_eq!(out.0, [RowName::Account, RowName::Model]);
        assert!(out.1.is_empty(), "{:?}", out.1);
    }

    #[test]
    fn footer_buttons_move_and_show_or_hide_the_selected_row() {
        let cur = config(None);
        let model = row_at(&cur, RowName::Model).0;
        let down = button(&cur, ("", Some(CTRL_S)));
        let up = button(&cur, ("", Some(CTRL_W)));
        let hide = button(&cur, ("", Some(' ')));
        let out = run_events(&cur, vec![press(model), press(down), key("Enter")]).unwrap().0;
        assert_eq!(out.get(3), Some(&RowName::Model), "down: {out:?}");
        let out = run_events(&cur, vec![press(model), press(up), key("Enter")]).unwrap().0;
        assert_eq!(out.get(1), Some(&RowName::Model), "up: {out:?}");
        let out = run_events(&cur, vec![press(model), press(hide), key("Enter")]).unwrap().0;
        assert!(!out.contains(&RowName::Model), "hidden: {out:?}");
        assert!(run_events(&cur, vec![press(button(&cur, ("Enter", None)))]).is_some());
        assert_eq!(run_events(&cur, vec![press(button(&cur, ("Escape", None)))]), None);
    }
}
