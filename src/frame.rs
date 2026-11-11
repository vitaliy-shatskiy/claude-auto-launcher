//! Frame pieces every screen shares (`Screens.ps1` middle): the footer as data, radio rows, click
//! spans, the row map, and the per-line truncate-then-paint chain. Pure.

use crate::layout::{display_width, limit_line};
use crate::theme::{self, Glyphs};

/// 50 columns: a phone over RDP. 16 rows: the worst launch frame at 50 columns plus one.
pub const MIN_WIDTH: usize = 50;
pub const MIN_HEIGHT: usize = 16;
pub const TWO_PANE_WIDTH: usize = 100;
pub const SHORT_PICKER_HEIGHT: usize = 30;

/// `Get-FrameWidth`: the last console column is never written (it wraps on some terminals).
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
        }
    }

    /// Kept on a narrow footer, beside the buttons.
    #[must_use]
    pub fn kept(self) -> Self {
        Self { keep: true, ..self }
    }

    pub fn key(token: &str, label: &str, key: &str) -> Self {
        Self {
            token: token.into(),
            label: label.into(),
            clickable: true,
            key: key.into(),
            ch: None,
            keep: false,
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

/// `New-HintFooter`: one list drives what is printed, where it can be clicked and which key a
/// click means. All hints on one line when they fit `width` (0 = never wrap); else the plain
/// (not clickable) hints drop, except the kept ones, and what is left goes on one line or into an
/// aligned grid with the most columns that fit. A hint is never split.
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
        if !h.label.is_empty() {
            piece.push(' ');
            piece.push_str(&h.label);
        }
        piece
    };
    let one_line =
        |hs: &[&Hint]| 2 + hs.iter().map(|h| display_width(&piece_of(h))).sum::<usize>() + display_width(&sep) * hs.len().saturating_sub(1);
    let all: Vec<&Hint> = hints.iter().collect();
    let buttons: Vec<&Hint> = hints.iter().filter(|h| h.clickable || h.keep).collect();
    let (shown, columns) = if width == 0 || one_line(&all) <= width || buttons.is_empty() {
        (all, hints.len().max(1))
    } else if one_line(&buttons) <= width {
        let n = buttons.len();
        (buttons, n)
    } else {
        let fits = |k: usize| 2 + column_widths(&buttons, k, &piece_of).iter().sum::<usize>() + display_width(&sep) * (k - 1) <= width;
        let most = (1..buttons.len()).rev().find(|k| fits(*k)).unwrap_or(1);
        // As few rows as the widest grid needs, filled evenly: 3 + 3 rather than 5 + 1.
        let even = buttons.len().div_ceil(buttons.len().div_ceil(most));
        let k = if fits(even) { even } else { most };
        (buttons, k)
    };
    let col_w = if shown.len() > columns {
        column_widths(&shown, columns, &piece_of)
    } else {
        Vec::new()
    };
    let mut lines = Vec::new();
    for (r, chunk) in shown.chunks(columns).enumerate() {
        let mut text = String::from("  ");
        let mut spans = Vec::new();
        let mut prev_w = 0;
        for (c, h) in chunk.iter().enumerate() {
            if c > 0 {
                let pad = col_w.get(c - 1).map_or(0, |w| w.saturating_sub(prev_w));
                text.push_str(&" ".repeat(pad));
                text.push_str(&sep);
            }
            prev_w = display_width(&piece_of(h));
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
            });
        }
        lines.push(FooterLine { text, spans });
    }
    Footer { lines }
}

/// The width of each grid column when `hints` fill rows of `k`, row-major.
fn column_widths(hints: &[&Hint], k: usize, piece_of: &dyn Fn(&Hint) -> String) -> Vec<usize> {
    let mut w = vec![0; k];
    for (i, h) in hints.iter().enumerate() {
        if let Some(slot) = w.get_mut(i % k) {
            *slot = (*slot).max(display_width(&piece_of(h)));
        }
    }
    w
}

/// Which footer button is lit: by (key, char), so a wrapped footer stays correct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FooterLight {
    pub hover: Option<(String, Option<char>)>,
    pub selected: Vec<(String, Option<char>)>,
}

/// `Get-FooterHover`: the (key, char) a hovered clickable-span INDEX names.
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

/// `Add-HintColor`: paints key tokens by COLUMN, so stripping the colour returns the line exactly.
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
            out.push_str(label_tint);
            out.push_str(&char_slice(&chars, cursor, label_end));
            out.push_str(theme::RESET);
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

/// `Measure-CellSpans`: columns in DISPLAY CELLS of pieces about to be joined.
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

/// Inputs of `New-RadioRow`.
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

/// `New-RadioRow`: `label  ○ a ● [b] ○ c` with a click cell per value. The compact form drops the
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
                if !spec.hover.is_empty() && v.eq_ignore_ascii_case(spec.hover) {
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
}

/// A finished frame: its lines and its row map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    pub lines: Vec<String>,
    pub map: RowMap,
}

/// A screen's body painter (`Add-LaunchColor`, `Add-PickerColor`), run after dim spans and before
/// the hover band.
pub type BodyPainter<'a> = &'a dyn Fn(&str, bool) -> String;

/// `Format-FrameLine`, body lines: truncate, dim spans, the body painter, the hover band LAST.
pub fn format_body_line(line: &str, width: usize, style: Style, painter: BodyPainter<'_>) -> String {
    let l = limit_line(line, width, style.ascii);
    let l = theme::add_dim_span_color(&l, style.color);
    let l = painter(&l, style.color);
    theme::add_hover_span_color(&l, style.color)
}

/// `Complete-PickerFrame` without the memo: appends the footer, records its spans, paints.
pub fn complete_frame(
    lines: &[String],
    footer: &Footer,
    width: usize,
    style: Style,
    painter: BodyPainter<'_>,
    light: &FooterLight,
    map: &mut RowMap,
) -> Vec<String> {
    let footer_index = lines.len();
    let mut out_lines: Vec<String> = lines.iter().map(|l| format_body_line(l, width, style, painter)).collect();
    let mut clickable = Vec::new();
    for fl in &footer.lines {
        let text = limit_line(&theme::add_dim_span_color(&fl.text, false), width, style.ascii);
        let len = i32::try_from(text.chars().count()).unwrap_or(i32::MAX);
        clickable.extend(fl.spans.iter().filter(|s| s.start >= 0 && s.end < len).cloned());
        out_lines.push(add_hint_color(&text, &fl.spans, style.color, light));
    }
    map.footer_y = Some(i32::try_from(footer_index).unwrap_or(0));
    map.footer = clickable;
    map.footer_lines = footer.lines.len();
    out_lines
}

/// `Get-TooSmallFrame`: the size line first, so the numbers survive any cut.
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
