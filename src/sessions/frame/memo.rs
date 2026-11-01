//! The frame memo (`Get-MemoFrame`): the last frame built, repainted only where a hover moved.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::PickerView;
use crate::frame::{Footer, FooterLight, Frame, RowMap, add_hint_color, footer_hover, format_body_line};
use crate::list::{add_hover_span_at, add_picker_color};
use crate::theme::Glyphs;

/// Where a hover band goes on one memoised row: line, first character, length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Band {
    pub(super) y: usize,
    pub(super) start: usize,
    pub(super) len: usize,
}

/// The last frame built, for the hover-only repaint (`Get-MemoFrame`).
#[derive(Debug, Clone, Default)]
pub struct Memo {
    pub(super) key: u64,
    pub(super) unpainted: Vec<String>,
    pub(super) painted: Vec<String>,
    pub(super) hover_row: i32,
    pub(super) hover: i32,
    pub(super) rows: Vec<(i32, Band)>,
    pub(super) footer: Option<Footer>,
    pub(super) footer_index: usize,
    pub(super) width: usize,
    pub(super) map: RowMap,
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
        v.scope,
        v.project_name,
    )
        .hash(&mut h);
    (v.clock.now_ms.div_euclid(60_000), v.clock.offset_minutes).hash(&mut h);
    for s in v.sessions {
        s.hash(&mut h);
    }
    h.finish()
}

pub(super) fn painter(g: Glyphs) -> impl Fn(&str, bool) -> String {
    move |l: &str, c: bool| add_picker_color(l, c, &g)
}

impl Memo {
    /// The memoised frame repainted for a new hover, or `None` to rebuild.
    pub(super) fn hit(&mut self, key: u64, v: &PickerView<'_>, g: Glyphs) -> Option<Frame> {
        if self.key != key || self.painted.is_empty() {
            return None;
        }
        let paint = painter(g);
        let mut painted = self.painted.clone();
        if self.hover_row != v.hover_row {
            for idx in [self.hover_row, v.hover_row] {
                let Some((_, b)) = self.rows.iter().find(|(r, _)| *r == idx) else {
                    continue;
                };
                let mut line = self.unpainted.get(b.y)?.clone();
                if idx == v.hover_row {
                    line = add_hover_span_at(&line, b.start, b.len)?;
                }
                *painted.get_mut(b.y)? = format_body_line(&line, self.width, v.style, &paint);
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
