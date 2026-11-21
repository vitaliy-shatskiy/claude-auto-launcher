//! The frame memo: the last frame built, repainted only where a hover moved.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::PickerView;
use crate::cockpit;
use crate::frame::{Footer, FooterLight, Frame, RowMap, add_hint_color, footer_hover};
use crate::list::add_hover_span_at;
use crate::theme::{add_select_band_color, add_tone_color, glyphs};

/// Where a hover band goes on one memoised row: line, first character, length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Band {
    pub(super) y: usize,
    pub(super) start: usize,
    pub(super) len: usize,
}

/// The last frame built, for the hover-only repaint.
#[derive(Debug, Clone, Default)]
pub struct Memo {
    pub(super) key: u64,
    /// The sessions panel's interior line on each page row, then the footer's plain lines.
    pub(super) unpainted: Vec<String>,
    pub(super) painted: Vec<String>,
    /// The chat panel's painted line on each page row (empty when stacked).
    pub(super) right: Vec<String>,
    pub(super) left_width: usize,
    pub(super) hover_row: i32,
    pub(super) hover: i32,
    pub(super) rows: Vec<(i32, Band)>,
    pub(super) footer: Option<Footer>,
    pub(super) footer_index: usize,
    pub(super) map: RowMap,
    pub(super) selected: i32,
}

pub(super) fn memo_key(v: &PickerView<'_>) -> u64 {
    let mut h = DefaultHasher::new();
    (
        v.width,
        v.height,
        v.style.color,
        v.style.ascii,
        v.index,
        v.filter,
        v.typing,
        v.scope,
        v.project_name,
    )
        .hash(&mut h);
    (v.clock.now_ms.div_euclid(60_000), v.clock.zone, v.chat).hash(&mut h);
    for s in v.sessions {
        s.hash(&mut h);
    }
    h.finish()
}

/// The picker's body painter: tone spans, then the selected-row band (the hover band comes last,
/// in `format_body_line`).
pub(super) fn painter() -> impl Fn(&str, bool) -> String {
    |l: &str, c: bool| add_select_band_color(&add_tone_color(l, c), c)
}

impl Memo {
    /// The memoised frame repainted for a new hover, or `None` to rebuild.
    pub(super) fn hit(&mut self, key: u64, v: &PickerView<'_>) -> Option<Frame> {
        if self.key != key || self.painted.is_empty() {
            return None;
        }
        let paint = painter();
        let mut painted = self.painted.clone();
        if self.hover_row != v.hover_row {
            for idx in [self.hover_row, v.hover_row] {
                // The selected row keeps its own band; a hover never repaints it.
                if idx == self.selected {
                    continue;
                }
                let Some((_, b)) = self.rows.iter().find(|(r, _)| *r == idx) else {
                    continue;
                };
                let mut line = self.unpainted.get(b.y)?.clone();
                if idx == v.hover_row {
                    line = add_hover_span_at(&line, b.start, b.len)?;
                }
                let panel = super::sessions_panel(String::new(), Vec::new(), self.left_width, 0, &paint, &glyphs(v.style.ascii));
                let right = self.right.get(b.y).map_or("", String::as_str);
                *painted.get_mut(b.y)? = format!("{}{right}", cockpit::line(&panel, &line, false, v.style));
            }
        }
        if self.hover != v.hover
            && let Some(footer) = &self.footer
        {
            let light = FooterLight {
                hover: footer_hover(footer, v.hover),
                selected: Vec::new(),
            };
            for (f, fl) in footer.lines.iter().enumerate() {
                let y = self.footer_index + f;
                *painted.get_mut(y)? = add_hint_color(self.unpainted.get(y)?, &fl.spans, v.style.color, &light);
            }
        }
        self.painted.clone_from(&painted);
        self.hover_row = v.hover_row;
        self.hover = v.hover;
        Some(Frame {
            lines: painted,
            map: self.map.clone(),
        })
    }
}
