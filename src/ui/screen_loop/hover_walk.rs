//! Test-only walks over a screen through the loop's own paths: a hover paints and never selects,
//! a first click only selects, a click on no target does nothing, every footer hint is alive.

use std::collections::VecDeque;

use super::{Cursor, Flow, Guards, MouseStep, Screen, mouse};
use crate::frame::Frame;
use crate::input::{MouseEvent, synthetic_key};
use crate::theme::{ACCENT_BG, BUTTON_BG};
use crate::ui::PreviewTerm;

/// Every terminal size a screen must draw without a panic: 0 and 1 cells, each side of the
/// minimum and of the two-column break, the common sizes, a 10000-column report; heights 0 to 40
/// and two tall ones.
pub(crate) fn every_size() -> impl Iterator<Item = (usize, usize)> {
    [0, 1, 49, 50, 119, 120, 140, 211, 280, 10_000]
        .into_iter()
        .flat_map(|w| (0..=40).chain([72, 85]).map(move |h| (w, h)))
}

/// Whether the cell at line `y` is the selected value, drawn `[value]`: it keeps its own paint
/// under the pointer.
fn is_selected_cell(frame: &Frame, row: i32, cell: &crate::frame::CellSpan) -> bool {
    let text: Vec<char> = usize::try_from(row)
        .ok()
        .and_then(|at| frame.lines.get(at))
        .map(|line| crate::theme::remove_ansi_color(line).chars().collect())
        .unwrap_or_default();
    let first = usize::try_from(cell.start).unwrap_or(0);
    let last = usize::try_from(cell.end).unwrap_or(0).min(text.len().saturating_sub(1));
    text.get(first..=last).is_some_and(|s| s.contains(&'['))
}

/// One point per clickable region: a row outside its cells (column 4), each cell but the selected
/// value, each button.
fn regions(f: &Frame) -> Vec<(i32, i32, String)> {
    let mut out = Vec::new();
    for r in &f.map.rows {
        out.push((4, r.y, format!("row {}", r.name)));
        out.extend(
            r.cells
                .iter()
                .filter(|c| !is_selected_cell(f, r.y, c))
                .map(|c| (c.start, r.y, format!("{} = {}", r.name, c.value))),
        );
    }
    if let Some(a) = &f.map.action {
        out.extend(
            a.cells
                .iter()
                .filter(|c| !is_selected_cell(f, a.y, c))
                .map(|c| (c.start, a.y, format!("action = {}", c.value))),
        );
    }
    if let Some(l) = &f.map.list {
        out.extend(l.row_ys.iter().map(|y| (6, *y, format!("list row at {y}"))));
    }
    if let Some(((a, _), rows)) = &f.map.side {
        out.extend(rows.iter().map(|r| (a + 2, r.y, format!("side row at {}", r.y))));
    }
    let top = f.map.footer_y.unwrap_or(0);
    out.extend(f.map.footer.iter().map(|s| {
        (
            s.start,
            top + i32::try_from(s.line).unwrap_or(0),
            format!("button {}{:?}", s.key, s.ch),
        )
    }));
    out
}

/// Panics naming the first region whose hover paints nothing or moves the selection. The screen
/// must draw in colour.
pub(crate) fn assert_every_region_hovers<S: Screen>(screen: &mut S, size: (usize, usize)) {
    let mut term = PreviewTerm::new(size, VecDeque::new(), false);
    let index = screen.cursor().index;
    let name = screen.name();
    screen.before();
    let base = screen.draw(size);
    let found = regions(&base);
    assert!(!found.is_empty(), "{name} has no clickable region");
    for (x, y, what) in found {
        *screen.cursor() = Cursor::at(index);
        let m = MouseEvent {
            x,
            y,
            is_move: true,
            ..MouseEvent::default()
        };
        let _ = mouse(screen, &mut term, &m, Some(&base.map), &mut Guards::default());
        screen.before();
        let after = screen.draw(size);
        let now = screen.cursor().index;
        assert_eq!(now, index, "{name}: a hover over {what} selected");
        let at = usize::try_from(y).unwrap_or(0);
        let (old, new) = (&base.lines[at], &after.lines[at]);
        assert!(
            new != old && (new.contains(BUTTON_BG) || new.contains(ACCENT_BG)),
            "{name}: a hover over {what} paints nothing: {new:?}"
        );
    }
    *screen.cursor() = Cursor::at(index);
}

/// What a filter hint's probe types: a filter that hides what is typed is a dead filter.
const FILTER_PROBE: &str = "qz";

fn plain(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| crate::theme::remove_ansi_color(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Panics naming the first clickable footer hint whose key, on a fresh screen from `make`, neither
/// ends the screen, nor changes the frame, nor moves `effects` (a count of what the screen's
/// callbacks did: a nested screen opened, a config write). A `filter` hint must also show what is
/// typed after it. The guard against functionality silently lost from a screen.
pub(crate) fn assert_every_hint_is_alive<S: Screen>(make: &mut dyn FnMut() -> S, size: (usize, usize), effects: &dyn Fn() -> usize) {
    let mut term = PreviewTerm::new(size, VecDeque::new(), false);
    let mut first = make();
    let name = first.name();
    first.before();
    let spans = first.draw(size).map.footer;
    assert!(!spans.is_empty(), "{name} has no footer button");
    for span in spans {
        let what = format!("{name}: the hint `{}{}`", span.key, span.ch.map(String::from).unwrap_or_default());
        let mut s = make();
        s.before();
        let base = s.draw(size);
        let before = effects();
        if let Flow::Done(_) = super::handle_key(&mut s, &mut term, &synthetic_key(&span.key, span.ch)) {
            continue;
        }
        s.before();
        let after = s.draw(size);
        assert!(
            after.lines != base.lines || effects() != before,
            "{what} is dead: its key changed nothing"
        );
        if span.ch == Some('/') {
            for c in FILTER_PROBE.chars() {
                let _ = super::handle_key(&mut s, &mut term, &crate::input::KeyPress::char(c));
                s.before();
            }
            let typed = plain(&s.draw(size).lines);
            assert!(typed.contains(FILTER_PROBE), "{what} hides what is typed:\n{typed}");
        }
    }
}

fn press(x: i32, y: i32) -> MouseEvent {
    MouseEvent {
        x,
        y,
        left: true,
        buttons: 1,
        ..MouseEvent::default()
    }
}

/// Points on no hit target: both edges of every line, the column past the frame, the top border,
/// and inside the right column on every line where two panels stand side by side.
fn stray_points(f: &Frame, width: usize) -> Vec<(i32, i32, String)> {
    let body = f.map.footer_y.map_or(f.lines.len(), |y| usize::try_from(y).unwrap_or(0));
    let w = i32::try_from(width).unwrap_or(0);
    let mut out = Vec::new();
    for (y, line) in f.lines.iter().take(body).enumerate() {
        let yi = i32::try_from(y).unwrap_or(0);
        out.push((0, yi, format!("the left edge of line {y}")));
        out.push((w - 1, yi, format!("the last column of line {y}")));
        let chars: Vec<char> = crate::theme::remove_ansi_color(line).chars().collect();
        // The left panel's right border, then the right panel's left border.
        // The right column's own rows (the workspace's sessions) are targets, not strays.
        let side = f.map.side.as_ref().is_some_and(|(_, rows)| rows.iter().any(|r| r.y == yi));
        if let Some(p) = chars
            .windows(2)
            .position(|c| "│╮╯".contains(c[0]) && "│╭╰".contains(c[1]))
            .filter(|_| !side)
        {
            out.push((i32::try_from(p + 4).unwrap_or(0), yi, format!("the right column of line {y}")));
        }
    }
    out.extend((0..w).step_by(5).map(|x| (x, 0, format!("the top border at {x}"))));
    out
}

/// Panics naming the first click off every target that changed the frame or ended the screen.
/// The screen must draw in colour (its borders are found by their glyphs).
pub(crate) fn assert_no_stray_click_acts<S: Screen>(screen: &mut S, size: (usize, usize)) {
    let mut term = PreviewTerm::new(size, VecDeque::new(), false);
    let name = screen.name();
    screen.before();
    let base = screen.draw(size);
    for (x, y, what) in stray_points(&base, size.0) {
        let step = mouse(screen, &mut term, &press(x, y), Some(&base.map), &mut Guards::default());
        assert!(!matches!(step, MouseStep::End(_)), "{name}: a click on {what} ended the screen");
        screen.before();
        let after = screen.draw(size);
        assert_eq!(after.lines, base.lines, "{name}: a click on {what} changed the frame");
    }
}

/// Panics naming the first target a single click acts on: a first click only selects.
pub(crate) fn assert_first_click_never_acts<S: Screen>(screen: &mut S, size: (usize, usize)) {
    let mut term = PreviewTerm::new(size, VecDeque::new(), false);
    let index = screen.cursor().index;
    let name = screen.name();
    screen.before();
    let base = screen.draw(size);
    let targets: Vec<(i32, i32, String)> = regions(&base).into_iter().filter(|(_, _, w)| !w.starts_with("button")).collect();
    for (x, y, what) in targets {
        *screen.cursor() = Cursor::at(index);
        let step = mouse(screen, &mut term, &press(x, y), Some(&base.map), &mut Guards::default());
        assert!(!matches!(step, MouseStep::End(_)), "{name}: one click on {what} acted");
        screen.before();
    }
    *screen.cursor() = Cursor::at(index);
}

/// Clicks `at` twice with one set of guards, as a user does; whether the first click ended the
/// screen, and what the second click ended it with (`None`: it did not, or with nothing).
pub(crate) fn click_twice<S: Screen>(screen: &mut S, size: (usize, usize), at: (i32, i32)) -> (bool, Option<S::Output>) {
    let mut term = PreviewTerm::new(size, VecDeque::new(), false);
    let mut g = Guards::default();
    let mut ends = Vec::new();
    for _ in 0..2 {
        screen.before();
        let map = screen.draw(size).map;
        ends.push(match mouse(screen, &mut term, &press(at.0, at.1), Some(&map), &mut g) {
            MouseStep::End(res) => Some(res),
            _ => None,
        });
    }
    let second = ends.pop().flatten().flatten();
    (ends.pop().flatten().is_some(), second)
}
