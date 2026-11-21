//! Draw, wait, resize, the mouse (hover paints only, click, double-click
//! guard, wheel, footer buttons), the arrows and WASD, Enter/Escape/Tab/hotkeys, and the log
//! records, written once. A screen is a `Screen` implementation around it.

use std::time::Instant;

use serde_json::{Map, Value, json};

use super::{Hit, Term, UiStop};
use crate::frame::{Area, Frame, RowMap};
use crate::input::{Event, KeyPress, is_hotkey, vk};
use crate::timing::{SpanId, Timings, view};

#[cfg(test)]
pub(crate) mod hover_walk;
mod mouse;
use mouse::mouse;

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
/// loop's own behaviour).
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
    /// A click on the row a previous click selected (a list row, or a launch-shaped row outside its
    /// cells); `None` = no handler, the click selects again.
    fn activate(&mut self, _hit: &Hit, _term: &mut dyn Term) -> Option<Flow<Self::Output>> {
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
    /// No input for one poll: `true` when background work changed what the screen shows.
    fn idle(&mut self) -> bool {
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
    acted_hit: Option<Hit>,
    acted_at: Option<u64>,
    footer_y: i32,
    footer_index: i32,
    footer_at: Option<u64>,
    /// An activation is fresh: the next ready point marks whether its twin already waits.
    settle: bool,
    /// The list row a click selected. Only a click on it activates: a cursor the keyboard or
    /// the wheel moved never starts anything on one click.
    selected: Option<Hit>,
}

impl Guards {
    fn disarm(&mut self) {
        self.selected = None;
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

/// Runs a screen to its end. `Err` when a scripted key source ran out (`KeysExhausted`) or the
/// console input could not be re-armed after a typed line (`ConsoleLost`).
pub fn run<S: Screen>(screen: &mut S, term: &mut dyn Term) -> Result<Option<S::Output>, UiStop> {
    let tm = term.logger().timings.clone();
    let span = tm.begin(&["screen", screen.name()]);
    let mut key_span = None;
    let r = run_loop(screen, term, &tm, &mut key_span);
    // The key ended the screen: no frame of this screen followed it, so it measured nothing.
    if let Some(k) = key_span {
        tm.discard(k);
    }
    tm.end(span);
    r
}

/// `key_span`: the open input-to-frame span, ended by this screen's next frame; dropped when a
/// nested screen ran first or the screen ended.
fn run_loop<S: Screen>(
    screen: &mut S,
    term: &mut dyn Term,
    tm: &Timings,
    key_span: &mut Option<SpanId>,
) -> Result<Option<S::Output>, UiStop> {
    let entered = Instant::now();
    let mut first_paint = Some(tm.begin(&["paint", screen.name()]));
    screen.before();
    log_screen(term, screen, "enter", entered);
    let mut need_draw = true;
    let mut map: Option<RowMap> = None;
    let mut g = Guards {
        footer_y: -1,
        footer_index: -1,
        settle: true,
        ..Guards::default()
    };
    let mut size = term.size();
    // A new screen never trusts what the terminal shows: its first paint writes every line.
    term.invalidate();
    let mut deferred = 0;
    // Only motion and keys coalesce: a click keeps its frame, which the twin guard waits on.
    let mut may_defer = false;
    loop {
        screen.before();
        // Input coalescing: while more input already waits, the frame waits for it, so a burst of
        // motion or key repeats costs one frame. The cap keeps a stream that never pauses (psmux
        // motion) from starving the screen.
        if need_draw && may_defer && map.is_some() && deferred < MAX_DEFERRED && term.input_pending() {
            deferred += 1;
        } else if need_draw {
            deferred = 0;
            size = term.size();
            let area = Area::of(size);
            let build = tm.begin(&[&format!("{}.frame.build", screen.name())]);
            let f = screen.draw(view::body_size(tm, area.size()));
            let lines = view::with_footer(tm, f.lines, screen.name(), area.size());
            tm.end(build);
            let write = tm.begin(&[&format!("{}.frame.write", screen.name())]);
            term.paint(&lines);
            tm.set_field(write, "bytes", u64::try_from(term.painted_bytes()).unwrap_or(u64::MAX));
            tm.end(write);
            map = Some(f.map);
            if let Some(p) = first_paint.take() {
                tm.end(p);
            }
            if let Some(k) = key_span.take() {
                tm.end_key(k);
            }
            need_draw = false;
        }
        // The twin test runs at the ready point, not at read time: a press already waiting once a
        // fresh activation's handler and frame are done is its twin, however slow they were.
        // Only a press arms or keeps the mark: psmux motion would keep it alive forever.
        if term.last_activation().is_some() {
            let press = term.press_pending();
            if let Some(a) = term.last_activation().as_mut() {
                a.queued = press && (g.settle || a.queued);
            }
        }
        g.settle = false;
        // An idle poll goes back to waiting unless a deferred frame is due or the screen has
        // something new to draw. Background work never runs while input waits: a key comes first.
        let ev = loop {
            match term.wait(size)? {
                Event::Idle if need_draw => break Event::Idle,
                Event::Idle if term.input_pending() || !screen.idle() => {}
                e => break e,
            }
        };
        may_defer = matches!(&ev, Event::Key(_)) || matches!(&ev, Event::Mouse(m) if m.is_move);
        let key = match ev {
            Event::Resize | Event::Idle => {
                need_draw = true;
                continue;
            }
            Event::Key(k) if tm.is_on() && k.is(vk::F12) => {
                view::show_timings(term, tm)?;
                term.invalidate();
                need_draw = true;
                continue;
            }
            Event::Key(k) => {
                // A key is a gesture of its own and disarms every twin guard.
                g.disarm();
                *term.last_activation() = None;
                // Coalesced keys share one frame: the first key's span measures to it.
                if tm.is_on() && key_span.is_none() {
                    *key_span = Some(tm.begin(&["key", screen.name()]));
                }
                k
            }
            Event::Mouse(m) => match mouse(screen, term, &m, map.as_ref(), &mut g) {
                MouseStep::Redraw(d) => {
                    // A frame an earlier event deferred stays due.
                    need_draw |= d;
                    continue;
                }
                MouseStep::End(r) => return Ok(finish(term, screen, entered, r)),
                MouseStep::Key(k) => k,
            },
        };
        need_draw = true;
        // Closed after the fact: a nested screen the key opens is not a child of it.
        let handled = tm.is_on().then(Instant::now);
        let flow = handle_key(screen, term, &key);
        if let Some(t0) = handled {
            tm.add(&[&format!("{}.key", screen.name())], None, t0, Instant::now(), None);
        }
        if let Flow::Done(r) = flow {
            return Ok(finish(term, screen, entered, r));
        }
    }
}

/// How many events in a row may defer a frame because more input waits.
const MAX_DEFERRED: u32 = 32;

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
mod tests;
