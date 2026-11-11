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
    fn draw(&mut self, size: (usize, usize)) -> Frame {
        let mut lines = vec![String::new(); 10];
        lines[0] = format!("{}x{}", size.0, size.1);
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
            lines,
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
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        self.q.pop_front().ok_or(UiStop::KeysExhausted)
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
    assert_eq!(run(&mut s, &mut term(Vec::new())), Err(UiStop::KeysExhausted));
}

#[test]
fn twin_window_has_a_lower_bound() {
    assert!(is_twin(Some(1100), Some(1000)));
    assert!(!is_twin(Some(900), Some(1000)), "a backwards clock is no twin");
    assert!(!is_twin(Some(1600), Some(1000)));
    assert!(!is_twin(None, Some(1)));
}

/// Records with their read stamp and the paint count at which each one sits in the queue: a
/// record is pending once that many frames were painted (a slow handler or first draw).
struct Clocked {
    inner: PreviewTerm,
    q: std::collections::VecDeque<(Event, u64, usize)>,
    now: u64,
    paints: usize,
}

impl Term for Clocked {
    fn size(&mut self) -> (usize, usize) {
        (80, 24)
    }
    fn paint(&mut self, lines: &[String]) {
        self.paints += 1;
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        let (ev, at, _) = self.q.pop_front().ok_or(UiStop::KeysExhausted)?;
        self.now = at;
        Ok(ev)
    }
    fn window_top(&mut self) -> i32 {
        0
    }
    fn input_pending(&mut self) -> bool {
        self.q.front().is_some_and(|e| self.paints >= e.2)
    }
    fn press_pending(&mut self) -> bool {
        let press = |e: &Event| matches!(e, Event::Mouse(m) if m.left && !m.is_move);
        self.q.iter().any(|e| self.paints >= e.2 && press(&e.0))
    }
    fn record_time(&self) -> Option<u64> {
        Some(self.now)
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

fn clocked(q: Vec<(Event, u64, usize)>) -> Clocked {
    Clocked {
        inner: PreviewTerm::new((80, 24), std::collections::VecDeque::default(), true),
        q: q.into(),
        now: 0,
        paints: 0,
    }
}

fn enter_at(at: u64) -> (Event, u64, usize) {
    (Event::Key(KeyPress::named(vk::ENTER)), at, 0)
}

fn row_one() -> List {
    List {
        c: Cursor::at(1),
        n: 3,
        acts: 0,
    }
}

/// D1: a click that selects row 1 first (one more frame painted), so the next click on it acts.
fn selected_first(q: Vec<(Event, u64, usize)>) -> Clocked {
    let mut all = vec![(click(5, 2), 100, 0)];
    all.extend(q.into_iter().map(|(e, at, paints)| (e, at, paints + 1)));
    clocked(all)
}

/// D1: the cursor sits on the row (keyboard or start), but one click only selects it.
#[test]
fn d1_a_click_on_a_cursor_row_no_click_selected_only_selects() {
    let mut s = row_one();
    let mut t = term(vec![click(5, 2), Event::Key(KeyPress::named(vk::ENTER))]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 0, "one click on a keyboard-placed cursor acted");
    let mut s = row_one();
    let mut t = term(vec![click(5, 2), click(5, 2), Event::Key(KeyPress::named(vk::ENTER))]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 1, "the second click on the selected row acts");
}

/// D1: a key between the two clicks drops the selection; keyboard Enter is unchanged.
#[test]
fn d1_a_key_between_two_clicks_drops_the_selection() {
    let mut s = List {
        c: Cursor::at(0),
        n: 3,
        acts: 0,
    };
    let mut t = term(vec![
        click(5, 2),
        Event::Key(KeyPress::named(vk::DOWN)),
        Event::Key(KeyPress::named(vk::UP)),
        click(5, 2),
        Event::Key(KeyPress::named(vk::ENTER)),
    ]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 0);
}

/// Debug mode (timings on, no footer) paints every frame exactly as off: one check for every
/// screen, since they all draw through this loop.
#[test]
fn debug_mode_draws_exactly_as_off() {
    let frames = |mut t: Events| {
        let mut s = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
        t.inner.frames.unwrap()
    };
    let keys = || vec![Event::Key(KeyPress::named(vk::DOWN)), Event::Key(KeyPress::named(vk::ENTER))];
    assert_eq!(frames(timed(keys())), frames(term(keys())));
}

/// CS-224 a: the twin queued during a handler slower than the window is read late. It is still
/// the twin, because it was waiting when the loop was ready again.
#[test]
fn cs224a_twin_queued_during_a_slow_activation_is_dropped() {
    let mut s = row_one();
    let mut t = selected_first(vec![(click(5, 2), 1000, 0), (click(5, 2), 1700, 2), enter_at(9000)]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 1, "the twin read 700 ms late acted again");
}

/// The control: a second press that arrives after the frame is up is a new gesture.
#[test]
fn cs224a_a_press_after_the_redraw_acts_again() {
    let mut s = row_one();
    let mut t = selected_first(vec![(click(5, 2), 1000, 0), (click(5, 2), 1700, 3), enter_at(9000)]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 2);
}

fn carried_activation(t: &mut Clocked) {
    *t.last_activation() = Some(Activation {
        x: 5,
        y: 2,
        at: Some(1000),
        queued: false,
    });
}

/// CS-224 a across screens: the previous screen's twin is already queued when this screen opens.
#[test]
fn cs224a_twin_queued_before_the_next_screen_opens_is_dropped() {
    let mut s = row_one();
    // A twin taken as a press would select row 1, and the later click would then act.
    let mut t = clocked(vec![(click(5, 2), 1700, 0), (click(5, 2), 9000, 0), enter_at(9500)]);
    carried_activation(&mut t);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 0);
}

/// CS-224 a across screens: the twin lands while the next screen's first frame is drawn.
#[test]
fn cs224a_twin_queued_during_the_next_screens_first_draw_is_dropped() {
    let mut s = row_one();
    let mut t = clocked(vec![(click(5, 2), 1700, 1), (click(5, 2), 9000, 0), enter_at(9500)]);
    carried_activation(&mut t);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 0, "the twin read after a slow first draw acted on the new screen");
}

/// CS-224 b: every later record of one gesture is dropped, not only the first twin.
#[test]
fn cs224b_third_and_fourth_records_of_one_gesture_are_dropped() {
    let mut s = row_one();
    let mut t = selected_first(vec![
        (click(5, 2), 1000, 0),
        (click(5, 2), 1700, 2),
        (click(5, 2), 1710, 2),
        (click(5, 2), 1720, 2),
        enter_at(9000),
    ]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 1);
}

fn hover(x: i32, y: i32) -> Event {
    Event::Mouse(MouseEvent {
        x,
        y,
        is_move: true,
        ..MouseEvent::default()
    })
}

/// Type-ahead keys after a carried activation, motion queued too: every key runs.
#[test]
fn type_ahead_keys_after_a_carried_activation_all_run() {
    let mut s = row_one();
    let k = |key: KeyPress| (Event::Key(key), 1700, 0);
    let mut t = clocked(vec![
        (hover(5, 2), 1690, 0),
        k(KeyPress::named(vk::DOWN)),
        k(KeyPress::named(vk::UP)),
        k(KeyPress::named(vk::UP)),
        k(KeyPress::named(vk::ENTER)),
    ]);
    carried_activation(&mut t);
    assert_eq!(run(&mut s, &mut t), Ok(Some(0)));
}

/// Motion records keep the queue full (psmux); a real press on the same cell after the redraw
/// still acts, because only a waiting button press marks the twin.
#[test]
fn a_real_click_behind_queued_motion_acts() {
    let mut s = row_one();
    let mut t = selected_first(vec![
        (click(5, 2), 1000, 0),
        (hover(5, 2), 1600, 2),
        (hover(5, 3), 1650, 2),
        (click(5, 2), 1700, 3),
        enter_at(9000),
    ]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 2, "motion records armed the twin mark");
}

/// The true twin, queued behind motion during the slow handler, is still dropped.
#[test]
fn a_twin_queued_behind_motion_during_the_handler_is_dropped() {
    let mut s = row_one();
    let mut t = selected_first(vec![
        (click(5, 2), 1000, 0),
        (hover(5, 2), 1600, 2),
        (click(5, 2), 1700, 2),
        enter_at(9000),
    ]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(1)));
    assert_eq!(s.acts, 1);
}

fn timed(events: Vec<Event>) -> Events {
    let mut t = term(events);
    let logger = crate::log::Logger {
        root: std::path::PathBuf::new(),
        run_id: String::new(),
        enabled: false,
        timings: crate::timing::Timings::on(Instant::now()),
    };
    t.inner = PreviewTerm::new((80, 24), std::collections::VecDeque::default(), true).with_logger(logger);
    t
}

fn logged(t: &Events) -> Vec<crate::timing::TimingRecord> {
    t.inner.logger().timings.take_unlogged()
}

#[test]
fn a_key_that_ends_its_screen_leaves_no_key_span() {
    let mut s = List {
        c: Cursor::at(0),
        n: 3,
        acts: 0,
    };
    let mut t = timed(vec![
        Event::Key(KeyPress::named(vk::DOWN)),
        Event::Key(KeyPress::named(vk::DOWN)),
        Event::Key(KeyPress::named(vk::ENTER)),
    ]);
    assert_eq!(run(&mut s, &mut t), Ok(Some(2)));
    let recs = logged(&t);
    assert_eq!(recs.iter().filter(|r| r.name == "key:list").count(), 2, "{recs:?}");
    assert!(recs.iter().all(|r| !r.aborted), "{recs:?}");
}

/// Opens a nested `List` on `u`, as the launch screen opens maintenance.
struct Outer {
    c: Cursor,
}

impl Screen for Outer {
    type Output = i32;
    fn name(&self) -> &'static str {
        "outer"
    }
    fn cursor(&mut self) -> &mut Cursor {
        &mut self.c
    }
    fn draw(&mut self, _size: (usize, usize)) -> Frame {
        Frame {
            lines: vec![String::new(); 3],
            map: RowMap::default(),
        }
    }
    fn rows(&self) -> Option<i32> {
        Some(3)
    }
    fn on_key(&mut self, key: &KeyPress, term: &mut dyn Term) -> bool {
        if key.ch != 'u' {
            return false;
        }
        let mut inner = List {
            c: Cursor::at(0),
            n: 3,
            acts: 0,
        };
        run(&mut inner, term).is_ok()
    }
}

#[test]
fn a_key_that_opens_a_nested_screen_is_dropped_and_the_footer_reads_its_own_screen() {
    let mut s = Outer { c: Cursor::at(0) };
    let mut t = timed(vec![
        Event::Key(KeyPress::char('u')),
        Event::Key(KeyPress::named(vk::DOWN)),
        Event::Key(KeyPress::named(vk::ESCAPE)),
        Event::Key(KeyPress::named(vk::DOWN)),
        Event::Key(KeyPress::named(vk::ESCAPE)),
    ]);
    assert_eq!(run(&mut s, &mut t), Ok(None));
    let timings = t.inner.logger().timings.clone();
    assert!(timings.latest_ms("key:outer").is_some());
    let recs = logged(&t);
    let keys: Vec<&str> = recs
        .iter()
        .filter(|r| r.name.starts_with("key:"))
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(keys, ["key:list", "key:outer"], "the u key spanned the nested screen: {recs:?}");
    let nested = recs.iter().find(|r| r.name == "screen:list").unwrap();
    assert_eq!(nested.parent.as_deref(), Some("screen:outer"));
}
