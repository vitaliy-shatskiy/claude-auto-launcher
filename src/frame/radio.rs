//! Radio rows: a label and its values on one line, the selected one marked, with the click cells
//! and the character spans of each value.

use super::{CellSpan, measure_cell_spans};
use crate::layout::display_width;
use crate::theme::{self, Glyphs};

/// Where a value's piece sits in CHARACTERS of the unmarked row (for a memo's band).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharSpan {
    pub value: String,
    pub start: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioRow {
    pub text: String,
    pub cells: Vec<CellSpan>,
    pub chars: Vec<CharSpan>,
    pub compact: bool,
}

/// Inputs of a radio row.
#[derive(Debug, Clone)]
pub struct RadioSpec<'a> {
    pub prefix: &'a str,
    pub label: &'a str,
    pub values: &'a [String],
    pub current: &'a str,
    /// The drawn text of each value, parallel to `values`.
    pub texts: &'a [String],
    pub label_width: usize,
    pub max_width: usize,
    pub hover: &'a str,
    pub compact: bool,
}

fn pad_right(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_owned()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

/// `label  ○ a ● [b] ○ c` with a click cell per value. The compact form drops the
/// glyphs; a value whose label carries a space has no readable compact form.
pub fn new_radio_row(spec: &RadioSpec<'_>, g: &Glyphs) -> RadioRow {
    let label_part = pad_right(&theme::strip_ctl(spec.label), spec.label_width);
    let text_of = |i: usize| theme::strip_ctl(spec.texts.get(i).map_or("", String::as_str));
    let build = |compact: bool| {
        let plain: Vec<String> = spec
            .values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let t = text_of(i);
                let cur = v.eq_ignore_ascii_case(spec.current);
                match (compact, cur) {
                    (true, true) => format!("[{t}]"),
                    (true, false) => t,
                    (false, true) => format!("{} [{t}]", g.on),
                    (false, false) => format!("{} {t}", g.off),
                }
            })
            .collect();
        let mut at = spec.prefix.chars().count() + label_part.chars().count();
        let chars = plain
            .iter()
            .zip(spec.values)
            .map(|(p, v)| {
                let span = CharSpan {
                    value: v.clone(),
                    start: at,
                    length: p.chars().count(),
                };
                at += p.chars().count() + 1;
                span
            })
            .collect();
        let pieces: Vec<String> = plain
            .iter()
            .zip(spec.values)
            .map(|(p, v)| {
                // The current value keeps its own paint under the pointer: a click that selected
                // it draws what the arrows draw.
                if !spec.hover.is_empty() && v.eq_ignore_ascii_case(spec.hover) && !v.eq_ignore_ascii_case(spec.current) {
                    theme::add_hover_span(p)
                } else {
                    p.clone()
                }
            })
            .collect();
        let cells = measure_cell_spans(&pieces, " ", display_width(&format!("{}{label_part}", spec.prefix)), spec.values);
        RadioRow {
            text: format!("{}{label_part}{}", spec.prefix, pieces.join(" ")),
            cells,
            chars,
            compact,
        }
    };
    let readable = (0..spec.values.len()).all(|i| !text_of(i).contains(' '));
    if spec.compact && readable {
        return build(true);
    }
    let full = build(false);
    if spec.max_width == 0 || display_width(&full.text) <= spec.max_width || !readable {
        return full;
    }
    build(true)
}
