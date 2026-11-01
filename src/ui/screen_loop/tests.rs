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
