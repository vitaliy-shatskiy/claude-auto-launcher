//! The wide launch page as a workspace: the launch panel and the limits on top, the projects panel
//! under the launch panel, and the right column following the focus (running and Claude Code
//! beside the launch rows; the selected project's facts and sessions beside the list). Exports
//! `HomePanel`; `home_frame` composes the page.

use super::{LaunchFrameInput, STATUS_HEIGHT, claude_code_lines, running_lines};
use crate::cockpit::{self, Panel};
use crate::frame::{self, Footer, FooterLight, Frame, MIN_HEIGHT, RowMap};
use crate::projects::view::{Home, HomeFocus};
use crate::theme;

/// The projects panel the launch page draws: which panel has focus, and the project list drawn
/// at a home layout, a width and a height (its own footer under it).
#[derive(Clone, Copy)]
pub(crate) struct HomePanel<'a> {
    pub focus: HomeFocus,
    pub draw: &'a dyn Fn(Home, usize, usize) -> Frame,
}

impl std::fmt::Debug for HomePanel<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HomePanel").field("focus", &self.focus).finish_non_exhaustive()
    }
}

/// Every hit of `map` moved `dy` rows down.
fn shift(map: &mut RowMap, dy: i32) {
    if let Some(l) = map.list.as_mut() {
        l.first_row_y += dy;
        for y in &mut l.row_ys {
            *y += dy;
        }
    }
    if let Some(a) = map.action.as_mut() {
        a.y += dy;
    }
    for r in map.rows.iter_mut().chain(map.side.iter_mut().flat_map(|s| s.1.iter_mut())) {
        r.y += dy;
    }
    if let Some(y) = map.footer_y.as_mut() {
        *y += dy;
    }
}

/// The project list's body exactly `rows` lines tall at `width`: its frame asked at the height that
/// leaves those rows above its footer.
fn list_body(h: HomePanel<'_>, home: Home, width: usize, rows: usize) -> Option<Frame> {
    let first = (h.draw)(home, width, (rows + 3).max(MIN_HEIGHT));
    let fl = first.map.footer_lines;
    let f = if first.map.footer_y == i32::try_from(rows).ok() {
        first
    } else {
        (h.draw)(home, width, rows + 1 + fl)
    };
    (f.map.footer_y == i32::try_from(rows).ok()).then_some(f)
}

/// The page under the `top` panels (launch and limits, `map` holding the launch rows).
pub(super) fn home_frame(
    inp: &LaunchFrameInput<'_>,
    h: HomePanel<'_>,
    top: Vec<String>,
    mut map: RowMap,
    (lw, rw): (usize, usize),
    footer: &Footer,
) -> Frame {
    let g = theme::glyphs(inp.style.ascii);
    let fw = frame::frame_width(inp.width);
    let top_h = top.len();
    let dy = i32::try_from(top_h).unwrap_or(0);
    map.hit_x = RowMap::panel_interior(0, lw);
    let mut lines = top;
    let home = Home {
        focus: h.focus,
        left_width: lw,
    };
    if h.focus != HomeFocus::Launch && inp.height.saturating_sub(top_h) >= MIN_HEIGHT {
        // Beside the list: its selected project's facts and sessions, its own footer.
        let mut f = (h.draw)(home, inp.width, inp.height - top_h);
        shift(&mut f.map, dy);
        lines.extend(f.lines);
        map.list = f.map.list;
        map.action = f.map.action;
        map.side = f.map.side;
        map.footer = f.map.footer;
        map.footer_y = f.map.footer_y;
        map.footer_lines = f.map.footer_lines;
        return Frame { lines, map };
    }
    let page_rows = inp.height.saturating_sub(1 + footer.lines.len());
    let rows = page_rows.saturating_sub(top_h);
    if rows > STATUS_HEIGHT
        // A frame keeps its last terminal column free: the list asks one more to stand `lw` wide.
        && let Some(f) = list_body(h, home, lw + 1, rows)
    {
        let body: Vec<String> = f.lines.into_iter().take(rows).collect();
        let running = Panel::plain(
            "running",
            running_lines(
                inp.running,
                rw.saturating_sub(2).min(cockpit::FACTS_WIDTH - 2),
                rows.saturating_sub(STATUS_HEIGHT + 2),
                &g,
            ),
            rw,
            rows - STATUS_HEIGHT,
        );
        let status = Panel::plain(
            "claude code",
            claude_code_lines(inp.version, inp.warnings, inp.mcp, &g),
            rw,
            STATUS_HEIGHT,
        );
        let mut right = cockpit::render(&running, inp.style);
        right.extend(cockpit::render(&status, inp.style));
        lines.extend(cockpit::beside(body, lw, &right));
        let mut fm = f.map;
        shift(&mut fm, dy);
        map.list = fm.list;
        map.action = fm.action;
        map.side = fm.side;
    } else if h.focus != HomeFocus::Launch {
        // No room for the focused list under the launch panel: it takes the page, as on a narrow
        // terminal, rather than keep the focus on a panel nobody sees.
        let page = Home {
            focus: h.focus,
            left_width: fw,
        };
        return (h.draw)(page, inp.width, inp.height);
    }
    let light = FooterLight {
        hover: frame::footer_hover(footer, inp.state.hover),
        selected: Vec::new(),
    };
    let lines = frame::append_footer(lines, footer, fw, inp.style, &light, &mut map);
    Frame { lines, map }
}
