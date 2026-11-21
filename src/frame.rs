//! Frame pieces every screen shares: the footer as data, radio rows, click
//! spans, the row map, and the per-line truncate-then-paint chain. Pure.

use crate::layout::{display_width, limit_line, new_titled_box};
use crate::theme::{self, Glyphs};

/// 50 columns: a phone over RDP. 16 rows: the worst launch frame at 50 columns plus one.
/// yagni: the compact launch page at 50x16 with a three-line footer fills all 16 rows (no headroom);
/// raise this to 17 when that one-row scroll is seen.
pub const MIN_WIDTH: usize = 50;
pub const MIN_HEIGHT: usize = 16;
pub const TWO_PANE_WIDTH: usize = 100;
pub const SHORT_PICKER_HEIGHT: usize = 30;

/// A reported side past this is a broken size report, not a terminal.
pub const MAX_REPORTED_CELLS: usize = 10_000;
/// What a broken side falls back to: the console's own default when its size read fails.
const FALLBACK_SIZE: (usize, usize) = (80, 25);

/// Where every screen draws: the whole terminal from its top-left cell, so terminal and frame
/// columns are the same numbers. A side of 0 or past `MAX_REPORTED_CELLS` takes `FALLBACK_SIZE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub width: usize,
    pub height: usize,
}

impl Area {
    pub fn of(size: (usize, usize)) -> Self {
        let sane = |n: usize, fallback: usize| if (1..=MAX_REPORTED_CELLS).contains(&n) { n } else { fallback };
        Self {
            width: sane(size.0, FALLBACK_SIZE.0),
            height: sane(size.1, FALLBACK_SIZE.1),
        }
    }

    pub fn size(self) -> (usize, usize) {
        (self.width, self.height)
    }
}

/// Blank lines after `lines` so the `below` lines that follow end on the page's last row: the
/// footer sits at the bottom of the area. The terminal's last row stays free (headroom: a frame
/// filling it scrolls the alternate buffer by one). A body taller than the page is left alone.
pub fn pad_to_bottom(lines: &mut Vec<String>, height: usize, below: usize) {
    let page = height.saturating_sub(1).saturating_sub(below);
    if lines.len() < page {
        lines.resize(page, String::new());
    }
}

/// The footer token of the keys that switch a row's value: `a/d ←/→` (`a/d <-/->` in ASCII).
pub fn switch_keys(g: &Glyphs) -> &'static str {
    if g.l_angle == '<' { "a/d <-/->" } else { "a/d \u{2190}/\u{2192}" }
}

/// The page every screen draws: `body` in a box from the top row down to the footer, `title`
/// (`<screen> ─ <facts>`) on the top border and `right` at its right end. A body taller than the
/// page is never cut here.
pub fn page(mut body: Vec<String>, size: (usize, usize), footer_lines: usize, title: &str, right: &str, style: Style) -> Vec<String> {
    let g = theme::glyphs(style.ascii);
    // Two borders, the footer and the headroom row.
    let rows = size.1.saturating_sub(3 + footer_lines);
    if body.len() < rows {
        body.resize(rows, String::new());
    }
    new_titled_box(&body, frame_width(size.0), title, right, &g, style.ascii)
}

/// Rows above the body on a header page: the box's three and the blank row under it.
pub const BODY_TOP: usize = 4;

/// The compact page (launch, maintenance, rows editor): a rounded header box across the frame
/// width with `title` at its left and the first of `rights` that fits at its right, a blank row,
/// the body, a blank row and a dim rule. The footer follows; nothing pads down to the bottom.
pub fn header_page(body: Vec<String>, width: usize, title: &str, rights: &[String], style: Style) -> Vec<String> {
    let g = theme::glyphs(style.ascii);
    let fw = frame_width(width);
    // The box's inner columns less one space each side.
    let text_w = fw.saturating_sub(4);
    let right = rights
        .iter()
        .find(|r| display_width(title) + 2 + display_width(r) <= text_w)
        .map_or("", String::as_str);
    let pad = text_w.saturating_sub(display_width(title) + display_width(right));
    let mut lines = new_titled_box(&[format!(" {title}{}{right}", " ".repeat(pad))], fw, "", "", &g, style.ascii);
    lines.push(String::new());
    lines.extend(body);
    lines.push(String::new());
    let rule: String = std::iter::repeat_n(g.h, text_w).collect();
    lines.push(format!("  {}{rule}{}", theme::DIM_OPEN, theme::DIM_CLOSE));
    lines
}

/// The column a header page's right-aligned row text ends at: under the header's right text.
pub fn right_edge(width: usize) -> usize {
    frame_width(width).saturating_sub(2)
}

/// `left`, then `right` ending at `edge`; with no room for both, `right` is left out.
pub fn align_right(left: &str, right: &str, edge: usize) -> String {
    let used = display_width(left) + display_width(right);
    if right.is_empty() || used + 2 > edge {
        return left.to_owned();
    }
    format!("{left}{}{right}", " ".repeat(edge - used))
}

/// The left padding of every page's text inside the frame: the cursor column and a space.
pub const PAD: &str = "   ";

/// The last console column is never written (it wraps on some terminals).
pub fn frame_width(width: usize) -> usize {
    width.saturating_sub(1)
}

/// Colour and glyph decisions for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub color: bool,
    pub ascii: bool,
}

mod footer;
pub use footer::{Footer, FooterLight, FooterLine, FooterSpan, Hint, add_hint_color, footer_hover, new_hint_footer};

/// One click cell: inclusive frame columns and the value it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSpan {
    pub start: i32,
    pub end: i32,
    pub value: String,
}

/// Columns in DISPLAY CELLS of pieces about to be joined.
pub fn measure_cell_spans(pieces: &[String], joiner: &str, start_x: usize, values: &[String]) -> Vec<CellSpan> {
    let mut x = i32::try_from(start_x).unwrap_or(0);
    let jw = i32::try_from(display_width(joiner)).unwrap_or(0);
    pieces
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = i32::try_from(display_width(p)).unwrap_or(0);
            let span = CellSpan {
                start: x,
                end: x + w - 1,
                value: values.get(i).cloned().unwrap_or_default(),
            };
            x += w + jw;
            span
        })
        .collect()
}

mod radio;
pub use radio::{CharSpan, RadioRow, RadioSpec, new_radio_row};

/// A launch-shaped row in the row map: its index, name, frame line and option cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowHit {
    pub index: usize,
    pub name: String,
    pub y: i32,
    pub cells: Vec<CellSpan>,
}

/// A list-shaped row map (picker, project screen): rows one per line from `first_row_y`, or at
/// the explicit `row_ys` when separators break the list up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListMap {
    pub first_row_y: i32,
    pub row_count: i32,
    pub start: i32,
    pub row_ys: Vec<i32>,
}

/// The project screen's action row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionHit {
    pub y: i32,
    pub cells: Vec<CellSpan>,
}

/// Where everything landed, filled by the code that drew it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowMap {
    pub rows: Vec<RowHit>,
    pub list: Option<ListMap>,
    pub action: Option<ActionHit>,
    pub footer_y: Option<i32>,
    pub footer: Vec<FooterSpan>,
    pub footer_lines: usize,
    /// The columns `[start, end)` every row, list and action hit lies in: the interior of the
    /// panel holding them. A click outside (a border, the column beside, past the frame) is no
    /// hit; `None` leaves the whole line clickable.
    pub hit_x: Option<(i32, i32)>,
    /// A second panel's rows (the workspace's sessions beside the list): hit only inside `.0`.
    pub side: Option<((i32, i32), Vec<RowHit>)>,
}

impl RowMap {
    /// The interior of a panel `width` cells wide whose left border is at column `left`.
    pub fn panel_interior(left: usize, width: usize) -> Option<(i32, i32)> {
        let a = i32::try_from(left + 1).ok()?;
        let b = i32::try_from((left + width).saturating_sub(1)).ok()?;
        Some((a, b))
    }
}

/// A finished frame: its lines and its row map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    pub lines: Vec<String>,
    pub map: RowMap,
}

/// A screen's body painter, run after dim spans and before
/// the hover band.
pub type BodyPainter<'a> = &'a dyn Fn(&str, bool) -> String;

/// Body lines: truncate, dim spans, the body painter, the hover band LAST.
pub fn format_body_line(line: &str, width: usize, style: Style, painter: BodyPainter<'_>) -> String {
    let l = limit_line(line, width, style.ascii);
    let l = theme::add_dim_span_color(&l, style.color);
    let l = painter(&l, style.color);
    theme::add_hover_span_color(&l, style.color)
}

/// The frame's end without the memo: appends the footer, records its spans, paints.
pub fn complete_frame(
    lines: &[String],
    footer: &Footer,
    width: usize,
    style: Style,
    painter: BodyPainter<'_>,
    light: &FooterLight,
    map: &mut RowMap,
) -> Vec<String> {
    let body = lines.iter().map(|l| format_body_line(l, width, style, painter)).collect();
    append_footer(body, footer, width, style, light, map)
}

/// Painted `lines` with the footer under them; the footer's row and clickable spans go to `map`.
pub fn append_footer(
    mut lines: Vec<String>,
    footer: &Footer,
    width: usize,
    style: Style,
    light: &FooterLight,
    map: &mut RowMap,
) -> Vec<String> {
    map.footer_y = Some(i32::try_from(lines.len()).unwrap_or(0));
    let mut clickable = Vec::new();
    for fl in &footer.lines {
        let text = limit_line(&theme::add_dim_span_color(&fl.text, false), width, style.ascii);
        let len = i32::try_from(text.chars().count()).unwrap_or(i32::MAX);
        clickable.extend(fl.spans.iter().filter(|s| s.start >= 0 && s.end < len).cloned());
        lines.push(add_hint_color(&text, &fl.spans, style.color, light));
    }
    map.footer = clickable;
    map.footer_lines = footer.lines.len();
    lines
}

/// The size line first, so the numbers survive any cut.
pub fn too_small_frame(width: usize, height: usize, ascii: bool) -> Vec<String> {
    let fw = frame_width(width);
    let mut lines: Vec<String> = [
        format!("  need {MIN_WIDTH}x{MIN_HEIGHT}, have {width}x{height}"),
        "  terminal too small".into(),
        String::new(),
        "  resize the window, or press esc".into(),
    ]
    .iter()
    .map(|l| limit_line(l, fw, ascii))
    .collect();
    lines.truncate(height.max(1));
    lines
}

#[cfg(test)]
mod tests;
