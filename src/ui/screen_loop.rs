//! `Invoke-ScreenLoop`: draw, wait, resize, the mouse (hover paints only, click, double-click
//! guard, wheel, footer buttons), the arrows and WASD, Enter/Escape/Tab/hotkeys, and the log
//! records, written once. A screen is a `Screen` implementation around it.

use std::time::Instant;

use serde_json::{Map, Value, json};

use super::{Activation, Exhausted, Hit, Term, hit_at};
use crate::frame::{Frame, RowMap};
use crate::input::{Event, KeyPress, MouseEvent, is_hotkey, synthetic_key, vk};

/// What a handler asks the loop to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow<R> {
    Continue,
    /// End the screen; `None` is a cancel (Escape's default).
    Done(Option<R>),
}

/// The loop's own state every screen carries: the cursor and the three hover fields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    pub index: i32,
    /// Footer button under the mouse (-1 none).
    pub hover: i32,
    /// List or launch row under the mouse (-1 none).
    pub hover_row: i32,
    /// Option value under the mouse ('' none).
    pub hover_value: String,
    /// A filter box owns every key while typing.
    pub typing: bool,
}

impl Cursor {
    pub fn at(index: i32) -> Self {
        Self {
            index,
            hover: -1,
            hover_row: -1,
            hover_value: String::new(),
            typing: false,
        }
    }
}

/// One screen: its draw and its handlers. Every handler is optional (the defaults are the
/// loop's own behaviour), exactly like the PowerShell handler table.
pub trait Screen {
    type Output;

    /// The `screen` name in the log records.
    fn name(&self) -> &'static str;
    fn cursor(&mut self) -> &mut Cursor;
    /// Draws at the given size; the row map comes from the same build that drew the rows.
    fn draw(&mut self, size: (usize, usize)) -> Frame;
    /// Runs once before the first draw and before every iteration.
    fn before(&mut self) {}
    /// The cursor bound; `None` = no cursor (no Up/Down, no WASD aliases).
    fn rows(&self) -> Option<i32> {
        None
    }
    /// `true` when handled; `false` takes the default (move the index).
    fn up(&mut self) -> bool {
        false
    }
    fn down(&mut self) -> bool {
        false
    }
    fn has_left_right(&self) -> bool {
        false
    }
    fn left(&mut self) -> Flow<Self::Output> {
        Flow::Continue
    }
    fn right(&mut self) -> Flow<Self::Output> {
        Flow::Continue
    }
    /// `None` = no Enter handler.
    fn enter(&mut self) -> Option<Flow<Self::Output>> {
        None
    }
    /// `None` = the default, which ends the screen with no result.
    fn escape(&mut self) -> Option<Flow<Self::Output>> {
        None
    }
    fn tab(&mut self) -> Option<Flow<Self::Output>> {
        None
    }
    /// The screen's hotkey letters (they disable the same-letter WASD alias).
    fn hotkeys(&self) -> &[char] {
        &[]
    }
    fn hotkey(&mut self, _c: char) -> Flow<Self::Output> {
        Flow::Continue
    }
    /// Called before arrow handling with every key; `true` consumed it (the loop redraws).
    fn on_key(&mut self, _key: &KeyPress, _term: &mut dyn Term) -> bool {
        false
    }
    /// While typing: `true` consumed the key.
    fn type_key(&mut self, _key: &KeyPress) -> bool {
        false
    }
    /// `true` when handled; `false` moves the index by the delta.
    fn wheel(&mut self, _delta: i32) -> bool {
        false
    }
    /// A click that is not an activation (selects).
    fn click(&mut self, _hit: &Hit) -> Flow<Self::Output> {
        Flow::Continue
    }
    /// A click on the row already under the cursor of a LIST screen; `None` = no handler.
    fn activate(&mut self, _hit: &Hit) -> Option<Flow<Self::Output>> {
        None
    }
    /// Extra fields on every record of this screen.
    fn log_fields(&self) -> Map<String, Value> {
        Map::new()
    }
    /// No screen/key records at all.
    fn silent(&self) -> bool {
        false
    }
}

/// Two records of one gesture land within a few ms; 500 ms is far under any human second click.
const TWIN_MS: u64 = 500;

fn is_twin(now: Option<u64>, at: Option<u64>) -> bool {
    matches!((now, at), (Some(n), Some(a)) if n >= a && n - a < TWIN_MS)
}

fn log<S: Screen + ?Sized>(term: &dyn Term, screen: &S, stage: &str, mut data: Map<String, Value>) {
    if screen.silent() {
        return;
    }
    for (k, v) in screen.log_fields() {
        data.insert(k, v);
    }
    term.logger().write(stage, data);
}

fn log_key<S: Screen>(term: &dyn Term, screen: &mut S, key: &str, extra: &[(&str, Value)]) {
    let mut d = Map::new();
    d.insert("screen".into(), json!(screen.name()));
    d.insert("key".into(), json!(key));
    d.insert("index".into(), json!(screen.cursor().index));
    for (k, v) in extra {
        d.insert((*k).into(), v.clone());
    }
    log(term, screen, "key", d);
}

fn log_screen<S: Screen>(term: &dyn Term, screen: &mut S, phase: &str, entered: Instant) {
    let mut d = Map::new();
    d.insert("name".into(), json!(screen.name()));
    d.insert("phase".into(), json!(phase));
    d.insert("rows".into(), json!(screen.rows().unwrap_or(0)));
    d.insert("index".into(), json!(screen.cursor().index));
    if phase == "leave" {
        d.insert("ms".into(), json!(u64::try_from(entered.elapsed().as_millis()).unwrap_or(u64::MAX)));
    }
    log(term, screen, "screen", d);
}

/// The twin guards of one screen run.
#[derive(Debug, Default)]
struct Guards {
    acted: bool,
    acted_row: i32,
    acted_at: Option<u64>,
    footer_y: i32,
    footer_index: i32,
    footer_at: Option<u64>,
}

impl Guards {
    fn disarm(&mut self) {
        self.acted = false;
        self.footer_y = -1;
        self.footer_index = -1;
        self.footer_at = None;
    }
}

enum MouseStep<R> {
    Redraw(bool),
    Key(KeyPress),
    End(Option<R>),
}

fn finish<S: Screen>(term: &dyn Term, screen: &mut S, entered: Instant, r: Option<S::Output>) -> Option<S::Output> {
    log_screen(term, screen, "leave", entered);
    r
}

/// Runs a screen to its end. `Err` only when a scripted key source ran out.
pub fn run<S: Screen>(screen: &mut S, term: &mut dyn Term) -> Result<Option<S::Output>, Exhausted> {
    let entered = Instant::now();
    screen.before();
    log_screen(term, screen, "enter", entered);
    let mut need_draw = true;
    let mut map: Option<RowMap> = None;
    let mut g = Guards {
        acted_row: -1,
        footer_y: -1,
        footer_index: -1,
        ..Guards::default()
    };
    let pending = term.input_pending();
    if let Some(a) = term.last_activation().as_mut() {
        a.queued = pending;
    }
    let mut size = term.size();
    loop {
        screen.before();
        if need_draw {
            size = term.size();
            let f = screen.draw(size);
            term.paint(&f.lines);
            map = Some(f.map);
        }
        need_draw = true;
        let pending = term.input_pending();
        if let Some(a) = term.last_activation().as_mut()
            && a.queued
            && !pending
        {
            a.queued = false;
        }
        let ev = term.wait(size)?;
        let key = match ev {
            Event::Resize => continue,
            Event::Key(k) => {
                // A key is a gesture of its own and disarms every twin guard.
                g.disarm();
                *term.last_activation() = None;
                k
            }
            Event::Mouse(m) => match mouse(screen, term, &m, map.as_ref(), &mut g) {
                MouseStep::Redraw(d) => {
                    need_draw = d;
                    continue;
                }
                MouseStep::End(r) => return Ok(finish(term, screen, entered, r)),
                MouseStep::Key(k) => k,
            },
        };
        if let Flow::Done(r) = handle_key(screen, term, &key) {
            return Ok(finish(term, screen, entered, r));
        }
    }
}

fn mouse<S: Screen>(screen: &mut S, term: &mut dyn Term, m: &MouseEvent, map: Option<&RowMap>, g: &mut Guards) -> MouseStep<S::Output> {
    let top = term.window_top();
    if m.wheel_up() || m.wheel_down() {
        let delta = if m.wheel_up() { -1 } else { 1 };
        if !screen.wheel(delta)
            && let Some(n) = screen.rows().filter(|n| *n > 0)
        {
            let c = screen.cursor();
            c.index = (c.index + delta).clamp(0, n - 1);
        }
        return MouseStep::Redraw(true);
    }
    let hit = hit_at(map, m.x, m.y, top);
    if m.is_move {
        // A move PAINTS, never selects.
        let btn = if let Hit::Footer { index, .. } = &hit {
            i32::try_from(*index).unwrap_or(-1)
        } else {
            -1
        };
        let (row, val) = match &hit {
            Hit::ListRow(r) => (*r, String::new()),
            Hit::Cell { row, value, .. } => (i32::try_from(*row).unwrap_or(-1), value.clone()),
            Hit::Action { value } => (-1, value.clone().unwrap_or_default()),
            Hit::None | Hit::Footer { .. } | Hit::LaunchRow { .. } => (-1, String::new()),
        };
        let c = screen.cursor();
        let changed = btn != c.hover || row != c.hover_row || val != c.hover_value;
        c.hover = btn;
        c.hover_row = row;
        c.hover_value = val;
        // Last move wins: a queued event will draw the frame this one would have.
        return MouseStep::Redraw(changed && !term.input_pending());
    }
    if !m.left {
        return MouseStep::Redraw(true);
    }
    let now = term.record_time();
    if let Some(last) = term.last_activation().take()
        && m.x == last.x
        && m.y == last.y
        && (m.is_double_click || last.queued || is_twin(now, last.at))
    {
        *term.last_activation() = Some(last);
        return MouseStep::Redraw(false);
    }
    if let Hit::Footer { span, index } = &hit {
        let idx = i32::try_from(*index).unwrap_or(-1);
        if m.is_double_click || (m.y == g.footer_y && idx == g.footer_index && is_twin(now, g.footer_at)) {
            return MouseStep::Redraw(false);
        }
        g.footer_y = m.y;
        g.footer_index = idx;
        g.footer_at = now;
        g.acted = false;
        return MouseStep::Key(synthetic_key(&span.key, span.ch));
    }
    let index = screen.cursor().index;
    if let Hit::ListRow(r) = hit
        && r == index
    {
        if g.acted && r == g.acted_row && (m.is_double_click || is_twin(now, g.acted_at)) {
            return MouseStep::Redraw(false);
        }
        *term.last_activation() = Some(Activation {
            x: m.x,
            y: m.y,
            at: now,
            queued: false,
        });
        if let Some(flow) = screen.activate(&hit) {
            g.acted = true;
            g.acted_row = r;
            g.acted_at = now;
            log_key(term, screen, "click", &[("button", json!("row"))]);
            if let Flow::Done(res) = flow {
                return MouseStep::End(res);
            }
            return MouseStep::Redraw(true);
        }
        *term.last_activation() = None;
    }
    if hit == Hit::None {
        g.acted = false;
        return MouseStep::Redraw(true);
    }
    g.acted = false;
    match screen.click(&hit) {
        Flow::Done(res) => MouseStep::End(res),
        Flow::Continue => MouseStep::Redraw(true),
    }
}

/// The keyboard path. `Done` ends the screen with that result.
fn handle_key<S: Screen>(screen: &mut S, term: &mut dyn Term, key: &KeyPress) -> Flow<S::Output> {
    if screen.on_key(key, term) {
        return Flow::Continue;
    }
    if screen.cursor().typing && screen.type_key(key) {
        return Flow::Continue;
    }
    let has_cursor = screen.rows().is_some();
    let alias = |s: &S, c: char| !s.hotkeys().contains(&c) && is_hotkey(key, c);
    let ended = |term: &dyn Term, screen: &mut S, label: &str, flow: Flow<S::Output>| {
        log_key(term, screen, label, &[]);
        flow
    };
    if key.is(vk::UP) || (has_cursor && alias(screen, 'w')) {
        if !screen.up() && screen.cursor().index > 0 {
            screen.cursor().index -= 1;
        }
        return Flow::Continue;
    }
    if key.is(vk::DOWN) || (has_cursor && alias(screen, 's')) {
        let n = screen.rows().unwrap_or(0);
        if !screen.down() && screen.cursor().index < n - 1 {
            screen.cursor().index += 1;
        }
        return Flow::Continue;
    }
    if screen.has_left_right() && (key.is(vk::LEFT) || key.is(vk::RIGHT) || alias(screen, 'a') || alias(screen, 'd')) {
        let back = key.is(vk::LEFT) || alias(screen, 'a');
        let flow = if back { screen.left() } else { screen.right() };
        return flow;
    }
    if key.is(vk::ENTER) {
        let flow = screen.enter().unwrap_or(Flow::Continue);
        return ended(term, screen, "Enter", flow);
    }
    if key.is(vk::ESCAPE) || key.is_ctrl(vk::C) {
        let flow = screen.escape().unwrap_or(Flow::Done(None));
        let label = if key.is(vk::ESCAPE) { "Escape" } else { "Ctrl+C" };
        return ended(term, screen, label, flow);
    }
    if key.is(vk::TAB) {
        if let Some(flow) = screen.tab() {
            return ended(term, screen, "Tab", flow);
        }
        return Flow::Continue;
    }
    let hot: Vec<char> = screen.hotkeys().to_vec();
    if let Some(c) = hot.into_iter().find(|c| is_hotkey(key, *c)) {
        let flow = screen.hotkey(c);
        return ended(term, screen, &c.to_string(), flow);
    }
    Flow::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{CellSpan, FooterSpan, ListMap};
    use crate::ui::PreviewTerm;

    /// A list of `n` rows; Enter and an activation return the index.
    struct List {
        c: Cursor,
        n: i32,
        acts: usize,
    }

    impl Screen for List {
        type Output = i32;
        fn name(&self) -> &'static str {
            "list"
        }
        fn cursor(&mut self) -> &mut Cursor {
            &mut self.c
        }
        fn draw(&mut self, _size: (usize, usize)) -> Frame {
            let footer = vec![FooterSpan {
                start: 2,
                end: 8,
                key_start: 2,
                key_end: 4,
                key: "Enter".into(),
                ch: None,
                line: 0,
            }];
            Frame {
                lines: vec![String::new(); 10],
                map: RowMap {
                    list: Some(ListMap {
                        first_row_y: 1,
                        row_count: self.n,
                        start: 0,
                        row_ys: Vec::new(),
                    }),
                    footer_y: Some(8),
                    footer,
                    action: Some(crate::frame::ActionHit {
                        y: 0,
                        cells: vec![CellSpan {
                            start: 0,
                            end: 3,
                            value: "new".into(),
                        }],
                    }),
                    ..RowMap::default()
                },
            }
        }
        fn rows(&self) -> Option<i32> {
            Some(self.n)
        }
        fn enter(&mut self) -> Option<Flow<i32>> {
            Some(Flow::Done(Some(self.c.index)))
        }
        fn click(&mut self, hit: &Hit) -> Flow<i32> {
            if let Hit::ListRow(r) = hit {
                self.c.index = *r;
            }
            Flow::Continue
        }
        fn activate(&mut self, _hit: &Hit) -> Option<Flow<i32>> {
            self.acts += 1;
            Some(Flow::Continue)
        }
    }

    /// Scripted events, mouse included.
    struct Events {
        inner: PreviewTerm,
        q: std::collections::VecDeque<Event>,
    }

    impl Term for Events {
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
            Some(1000)
        }
        fn last_activation(&mut self) -> &mut Option<Activation> {
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

    fn term(events: Vec<Event>) -> Events {
        Events {
            inner: PreviewTerm::new((80, 24), std::collections::VecDeque::default(), true),
            q: events.into(),
        }
    }

    fn click(x: i32, y: i32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            left: true,
            buttons: 1,
            ..MouseEvent::default()
        })
    }

    #[test]
    fn arrows_and_wasd_move_then_enter_returns() {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        let mut t = term(vec![
            Event::Key(KeyPress::named(vk::DOWN)),
            Event::Key(KeyPress::char('s')),
            Event::Key(KeyPress::char('s')),
            Event::Key(KeyPress::char('w')),
            Event::Key(KeyPress::named(vk::ENTER)),
        ]);
        assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    }

    #[test]
    fn escape_and_ctrl_c_cancel() {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        assert_eq!(run(&mut s, &mut term(vec![Event::Key(KeyPress::named(vk::ESCAPE))])), Ok(None));
        let ctrl_c = KeyPress {
            vk: vk::C,
            ctrl: true,
            ch: '\u{3}',
            ..KeyPress::default()
        };
        assert_eq!(run(&mut s, &mut term(vec![Event::Key(ctrl_c)])), Ok(None));
    }

    #[test]
    fn hover_paints_but_never_selects() {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        let mv = Event::Mouse(MouseEvent {
            x: 5,
            y: 3,
            is_move: true,
            ..MouseEvent::default()
        });
        let mut t = term(vec![mv, Event::Key(KeyPress::named(vk::ENTER))]);
        assert_eq!(run(&mut s, &mut t), Ok(Some(0)));
        assert_eq!(s.c.hover_row, 2);
    }

    #[test]
    fn click_selects_second_click_activates_twin_is_swallowed() {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        let mut t = term(vec![click(5, 2), click(5, 2), click(5, 2), Event::Key(KeyPress::named(vk::ENTER))]);
        assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
        assert_eq!(s.acts, 1, "the third record of one gesture does not act again");
    }

    #[test]
    fn footer_click_becomes_its_key() {
        let mut s = List {
            c: Cursor::at(2),
            n: 3,
            acts: 0,
        };
        let mut t = term(vec![click(3, 8)]);
        assert_eq!(run(&mut s, &mut t), Ok(Some(2)));
    }

    #[test]
    fn wheel_moves_the_cursor_clamped() {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        let down = Event::Mouse(MouseEvent {
            wheel: -128,
            ..MouseEvent::default()
        });
        let mut t = term(vec![down, down, down, Event::Key(KeyPress::named(vk::ENTER))]);
        assert_eq!(run(&mut s, &mut t), Ok(Some(2)));
    }

    #[test]
    fn exhausted_script_is_an_error() {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        assert_eq!(run(&mut s, &mut term(Vec::new())), Err(Exhausted));
    }

    #[test]
    fn twin_window_has_a_lower_bound() {
        assert!(is_twin(Some(1100), Some(1000)));
        assert!(!is_twin(Some(900), Some(1000)), "a backwards clock is no twin");
        assert!(!is_twin(Some(1600), Some(1000)));
        assert!(!is_twin(None, Some(1)));
    }
}
