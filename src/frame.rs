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

/// A clickable or plain hint of the footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub token: String,
    pub label: String,
    pub clickable: bool,
    /// The `ConsoleKey` name a click stands in for ('' when `ch` carries it).
    pub key: String,
    pub ch: Option<char>,
    /// A plain hint that stays when a narrow footer drops the plain ones.
    pub keep: bool,
    /// Drawn after the label in yellow (`⚠ 2`): it is part of the click span.
    pub badge: String,
}

impl Hint {
    pub fn plain(token: &str, label: &str) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: false,
            key: String::new(),
            ch: None,
            keep: false,
            badge: String::new(),
        }
    }

    /// Kept on a narrow footer, beside the buttons.
    #[must_use]
    pub fn kept(self) -> Self {
        Self { keep: true, ..self }
    }

    /// With a yellow badge after the label; an empty badge draws nothing.
    #[must_use]
    pub fn badged(self, badge: String) -> Self {
        Self { badge, ..self }
    }

    pub fn key(token: &str, label: &str, key: &str) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: true,
            key: key.into(),
            ch: None,
            keep: false,
            badge: String::new(),
        }
    }

    pub fn ch(token: &str, label: &str, ch: char) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: true,
            key: String::new(),
            ch: Some(ch),
            keep: false,
            badge: String::new(),
        }
    }
}

/// Columns of one hint on its footer line. `start`/`end` cover token and label (-1 when the hint
/// is not clickable); `key_start`/`key_end` the token alone. Character columns of the plain text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterSpan {
    pub start: i32,
    pub end: i32,
    pub key_start: usize,
    pub key_end: usize,
    pub key: String,
    pub ch: Option<char>,
    pub line: usize,
    /// Characters at the end of the span drawn as the yellow badge.
    pub badge: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLine {
    pub text: String,
    pub spans: Vec<FooterSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    pub lines: Vec<FooterLine>,
}

/// One list drives what is printed, where it can be clicked and which key a
/// click means. All hints on one line when they fit `width` (0 = never drop); else the plain
/// (not clickable) hints drop, except the kept ones, then the buttons by priority until the rest
/// fits. The footer is always one row: a hint is never split or wrapped.
pub fn new_hint_footer(hints: &[Hint], g: &Glyphs, width: usize, plain: bool) -> Footer {
    let sep = format!("  {}  ", g.hint_sep);
    let token_of = |h: &Hint| {
        if !h.clickable {
            h.token.clone()
        } else if plain {
            format!("[{}]", h.token)
        } else {
            format!(" {} ", h.token)
        }
    };
    let piece_of = |h: &Hint| {
        let mut piece = token_of(h);
        for part in [&h.label, &h.badge] {
            if !part.is_empty() {
                piece.push(' ');
                piece.push_str(part);
            }
        }
        piece
    };
    let one_line =
        |hs: &[&Hint]| 2 + hs.iter().map(|h| display_width(&piece_of(h))).sum::<usize>() + display_width(&sep) * hs.len().saturating_sub(1);
    let mut shown: Vec<&Hint> = hints.iter().collect();
    if width > 0 && one_line(&shown) > width && shown.iter().any(|h| h.clickable || h.keep) {
        shown.retain(|h| h.clickable || h.keep);
    }
    // Then by priority, the last listed first within a priority.
    for tier in 0..4 {
        while width > 0 && shown.len() > 1 && one_line(&shown) > width {
            let Some(i) = shown.iter().rposition(|h| priority(h) == tier) else {
                break;
            };
            shown.remove(i);
        }
    }
    let mut lines = Vec::new();
    {
        let r = 0;
        let mut text = String::from("  ");
        let mut spans = Vec::new();
        for (c, h) in shown.iter().enumerate() {
            if c > 0 {
                text.push_str(&sep);
            }
            let token = token_of(h);
            let start = text.chars().count();
            let token_end = start + token.chars().count() - 1;
            text.push_str(&piece_of(h));
            let end = text.chars().count() - 1;
            let col = |v: usize| i32::try_from(v).unwrap_or(i32::MAX);
            spans.push(FooterSpan {
                start: if h.clickable { col(start) } else { -1 },
                end: if h.clickable { col(end) } else { -1 },
                key_start: start,
                key_end: token_end,
                key: if h.clickable { h.key.clone() } else { String::new() },
                ch: if h.clickable { h.ch } else { None },
                line: r,
                badge: h.badge.chars().count(),
            });
        }
        lines.push(FooterLine { text, spans });
    }
    Footer { lines }
}

/// The order a narrow footer drops hints in: a button first (0), then a kept plain hint (the
/// move and switch keys, 1), tab, the filter and enter (2), esc last (3): the way back outlasts tab.
fn priority(h: &Hint) -> u8 {
    if !h.clickable {
        1
    } else if h.key == "Escape" {
        3
    } else if matches!(h.key.as_str(), "Enter" | "Tab") || h.ch == Some('/') {
        2
    } else {
        0
    }
}

/// Which footer button is lit: by (key, char), so a wrapped footer stays correct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FooterLight {
    pub hover: Option<(String, Option<char>)>,
    pub selected: Vec<(String, Option<char>)>,
}

/// The (key, char) a hovered clickable-span INDEX names.
pub fn footer_hover(footer: &Footer, hover: i32) -> Option<(String, Option<char>)> {
    let i = usize::try_from(hover).ok()?;
    footer
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .filter(|s| s.start >= 0)
        .nth(i)
        .map(|s| (s.key.clone(), s.ch))
}

fn char_slice(chars: &[char], from: usize, to_inclusive: usize) -> String {
    chars.get(from..=to_inclusive).map_or_else(String::new, |s| s.iter().collect())
}

/// Paints key tokens by COLUMN, so stripping the colour returns the line exactly.
pub fn add_hint_color(line: &str, spans: &[FooterSpan], enabled: bool, light: &FooterLight) -> String {
    if !enabled || line.is_empty() || spans.is_empty() {
        return line.to_owned();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut sorted: Vec<&FooterSpan> = spans.iter().collect();
    sorted.sort_by_key(|s| s.key_start);
    let mut out = String::new();
    let mut cursor = 0usize;
    for s in sorted {
        if s.key_start < cursor || s.key_end >= chars.len() {
            continue;
        }
        out.push_str(theme::DIM);
        out.push_str(
            &chars
                .get(cursor..s.key_start)
                .map_or_else(String::new, |c| c.iter().collect::<String>()),
        );
        out.push_str(theme::RESET);
        let id = (s.key.clone(), s.ch);
        let hovered = s.start >= 0 && light.hover.as_ref() == Some(&id);
        let selected = s.start >= 0 && light.selected.contains(&id);
        let cap = if hovered || selected {
            format!("{}{}", theme::ACCENT_BG, theme::ACCENT_FG)
        } else if s.start >= 0 {
            format!("{}{}", theme::BUTTON_BG, theme::BUTTON_FG)
        } else {
            theme::DIM.to_owned()
        };
        let label_tint = if hovered || selected { theme::BOLD } else { theme::DIM };
        out.push_str(&cap);
        out.push_str(&char_slice(&chars, s.key_start, s.key_end));
        out.push_str(theme::RESET);
        cursor = s.key_end + 1;
        if let Ok(end) = usize::try_from(s.end)
            && end >= cursor
            && cursor < chars.len()
        {
            let label_end = end.min(chars.len() - 1);
            let badge_at = (label_end + 1).saturating_sub(s.badge).max(cursor);
            out.push_str(label_tint);
            out.push_str(
                &chars
                    .get(cursor..badge_at)
                    .map_or_else(String::new, |c| c.iter().collect::<String>()),
            );
            out.push_str(theme::RESET);
            if badge_at <= label_end {
                out.push_str(theme::YELLOW);
                out.push_str(&char_slice(&chars, badge_at, label_end));
                out.push_str(theme::RESET);
            }
            cursor = label_end + 1;
        }
    }
    if cursor < chars.len() {
        out.push_str(theme::DIM);
        out.push_str(&chars.get(cursor..).map_or_else(String::new, |c| c.iter().collect::<String>()));
        out.push_str(theme::RESET);
    }
    out
}

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

/// The row index a workspace session line has in the row map: `SESSION_ROW + i`, past every
/// launch row.
pub const SESSION_ROW: usize = 1000;

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
