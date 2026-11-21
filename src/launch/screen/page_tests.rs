//! The launch screen through the screen loop as a full page: a resize redraws once at the new
//! size, clicks land on the rows and the footer where they are drawn, the area's offset included.

#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use std::collections::VecDeque;

use super::*;
use crate::input::{Event, MouseEvent};
use crate::launch::tests::four;
use crate::layout::display_width;
use crate::ui::screen_loop::run;
use crate::ui::{Activation, PreviewTerm, UiStop};

fn view() -> LaunchView {
    LaunchView {
        limits: Limits::new(),
        version: VersionInfo::default(),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: BTreeMap::new(),
        recent: Vec::new(),
        running: Vec::new(),
        mcp: String::new(),
        style: Style {
            color: false,
            ascii: false,
        },
    }
}

/// Scripted events; an event paired with a size switches the terminal to it first.
struct Resizing {
    inner: PreviewTerm,
    size: (usize, usize),
    q: VecDeque<(Event, Option<(usize, usize)>)>,
}

impl Term for Resizing {
    fn size(&mut self) -> (usize, usize) {
        self.size
    }
    fn paint(&mut self, lines: &[String]) {
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        let (e, size) = self.q.pop_front().ok_or(UiStop::KeysExhausted)?;
        if let Some(s) = size {
            self.size = s;
        }
        Ok(e)
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

fn term(size: (usize, usize), q: Vec<(Event, Option<(usize, usize)>)>) -> Resizing {
    Resizing {
        inner: PreviewTerm::new(size, VecDeque::new(), true),
        size,
        q: q.into(),
    }
}

fn click(x: i32, y: i32) -> (Event, Option<(usize, usize)>) {
    let m = MouseEvent {
        x,
        y,
        left: true,
        buttons: 1,
        ..MouseEvent::default()
    };
    (Event::Mouse(m), None)
}

fn key(vk: u16) -> (Event, Option<(usize, usize)>) {
    (Event::Key(KeyPress::named(vk)), None)
}

/// Runs a fresh launch screen over `t`; the frame it would draw at `size` comes back too.
fn drive(t: &mut Resizing, size: (usize, usize)) -> (Option<LaunchState>, Frame) {
    let mut r = four();
    let v = view();
    let p = Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut s = LaunchScreen::new(&mut r, st, &v, &p, 0, &mut hook);
    let f = s.draw(size);
    (run(&mut s, t).unwrap(), f)
}

/// The frame a fresh screen draws at `size`, for the click coordinates.
fn frame_at(size: (usize, usize)) -> Frame {
    drive(&mut term(size, vec![key(vk::ESCAPE)]), size).1
}

fn cell(f: &Frame, row: &str, value: &str) -> (i32, i32) {
    let hit = f.map.rows.iter().find(|h| h.name == row).unwrap();
    (hit.cells.iter().find(|c| c.value == value).unwrap().start, hit.y)
}

fn esc_button(f: &Frame) -> (i32, i32) {
    let span = f.map.footer.iter().find(|s| s.key == "Escape").unwrap();
    (span.start, f.map.footer_y.unwrap())
}

#[test]
fn a_resize_redraws_once_at_the_new_size() {
    let mut t = term((100, 30), vec![(Event::Resize, Some((160, 40))), key(vk::ESCAPE)]);
    drive(&mut t, (100, 30));
    let frames = t.inner.frames.unwrap();
    assert_eq!(frames.len(), 2, "one frame per size");
    assert_eq!(display_width(&frames[0][0]), 99);
    let widest = frames[1].iter().map(|l| display_width(l)).max().unwrap();
    assert!(
        widest > 99,
        "the second frame is laid out for the new width (two columns): {widest}"
    );
}

#[test]
fn at_160x40_a_click_selects_the_drawn_cell_and_the_footer_acts_only_where_drawn() {
    let f = frame_at((160, 40));
    let (mx, my) = cell(&f, "Effort", "max");
    let (ex, ey) = esc_button(&f);
    // The row under the footer draws nothing: a click there does nothing.
    let old_footer = ey + 1;
    let mut t = term((160, 40), vec![click(mx, my), click(ex, old_footer), key(vk::ENTER)]);
    let out = drive(&mut t, (160, 40)).0.unwrap();
    assert_eq!(out.effort, "max", "the click landed on the drawn cell");
    let mut t = term((160, 40), vec![click(ex, ey), key(vk::ENTER)]);
    assert!(drive(&mut t, (160, 40)).0.is_none(), "esc at the drawn footer quits");
}

#[test]
fn on_a_huge_terminal_clicks_count_from_the_top_left_page() {
    let f = frame_at((400, 150));
    let (mx, my) = cell(&f, "Effort", "max");
    let mut t = term((400, 150), vec![click(mx, my), key(vk::ENTER)]);
    assert_eq!(drive(&mut t, (400, 150)).0.unwrap().effort, "max", "the page starts at column 0");
}
