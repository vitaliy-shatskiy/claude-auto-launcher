//! Test-only: walks every clickable region of a screen's row map (row labels, value cells, footer
//! buttons) through the loop's own mouse path and proves a hover paints and never selects.

use std::collections::VecDeque;

use super::{Cursor, Guards, Screen, mouse};
use crate::frame::Frame;
use crate::input::MouseEvent;
use crate::theme::{ACCENT_BG, BUTTON_BG};
use crate::ui::PreviewTerm;

/// One point per clickable region: a row outside its cells (column 4), each cell, each button.
fn regions(f: &Frame) -> Vec<(i32, i32, String)> {
    let mut out = Vec::new();
    for r in &f.map.rows {
        out.push((4, r.y, format!("row {}", r.name)));
        out.extend(r.cells.iter().map(|c| (c.start, r.y, format!("{} = {}", r.name, c.value))));
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
