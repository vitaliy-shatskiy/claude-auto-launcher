//! The picker frame (`Get-PickerFrame` and its helpers): master-detail at >= 100 columns, one
//! column below with the preview underneath, and the frame memo that repaints only the lines a
//! hover change touched.

mod card;
mod chat;
mod memo;
pub(super) mod plain;
mod text;

use super::listing::Session;
use super::transcript::clean_text;
use crate::frame::{
    Footer, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RowMap, SHORT_PICKER_HEIGHT, Style, TWO_PANE_WIDTH, complete_frame,
    footer_hover, frame_width, new_hint_footer, too_small_frame,
};
use crate::layout::{join_panes, limit_line, new_box, viewport};
use crate::list::{ListRow, new_list_row, row_band_length};
use crate::theme::{self, Glyphs, glyphs};

use card::Preview;
pub use chat::ChatView;
pub use memo::Memo;
use memo::{Band, memo_key, painter};
pub use text::{local_stamp, relative_age};

/// The clock a frame is drawn against: now, and the local offset from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Clock {
    pub now_ms: i64,
    pub offset_minutes: i32,
}

/// `none` = no project context (no tab hint); `project` = the pool is one project; `all`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickerScope {
    None,
    Project,
    All,
}

/// Everything one picker frame is drawn from.
#[derive(Debug, Clone)]
pub struct PickerView<'a> {
    pub sessions: &'a [&'a Session],
    pub index: usize,
    pub filter: &'a str,
    pub width: usize,
    pub height: usize,
    pub clock: Clock,
    pub style: Style,
    pub scope: PickerScope,
    pub project_name: &'a str,
    /// Footer button index under the mouse, -1 none.
    pub hover: i32,
    /// List row under the mouse, -1 none.
    pub hover_row: i32,
    /// The highlighted session's chat for the preview pane.
    pub chat: ChatView<'a>,
}

/// `Select-ResumableSessions`' rule: not headless and something was asked.
pub fn resumable(s: &Session) -> bool {
    !s.headless && s.prompt_count > 0
}

/// `Select-SessionMatch`: a case-insensitive substring over project, worktree and the texts.
pub fn matches(s: &Session, filter: &str) -> bool {
    let f = filter.trim();
    if f.is_empty() {
        return true;
    }
    let hay = format!("{} {} {} {} {}", s.project, s.worktree, s.title, s.last_user, s.last_assistant);
    hay.to_lowercase().contains(&f.to_lowercase())
}

struct RowCtx<'a> {
    index: usize,
    hover_row: i32,
    scope: PickerScope,
    clock: Clock,
    g: &'a Glyphs,
    width: usize,
    ascii: bool,
}

/// `New-SessionRow`.
fn session_row(it: &Session, row_index: usize, c: &RowCtx<'_>) -> String {
    let mark = if row_index == c.index {
        format!(" {} ", c.g.cursor)
    } else {
        "   ".to_owned()
    };
    let mut place = if c.scope == PickerScope::Project {
        String::new()
    } else {
        it.project.clone()
    };
    if !it.worktree.is_empty() {
        place = format!("{} {}", c.g.worktree, it.worktree);
    }
    // A session of another account names its owner first: resuming it copies the transcript.
    if !it.account.is_empty() {
        format!("[{}] {place}", it.account).trim_end().clone_into(&mut place);
    }
    let age = relative_age(it.modified_ms, c.clock.now_ms);
    // A session written to within the minute is live now: a green dot before its title.
    let live = if age == "now" { format!("{} ", c.g.on) } else { String::new() };
    let what = [&it.title, &it.last_user]
        .into_iter()
        .find(|t| !t.is_empty())
        .cloned()
        .unwrap_or_default();
    let n = |s: &str| i64::try_from(s.chars().count()).unwrap_or(0);
    let room = i64::try_from(c.width).unwrap_or(0) - n(&mark) - n(&age) - n(&live) - 2;
    let mut label = format!("{live}{place}");
    if !what.is_empty() && room > n(&place) + 4 {
        let sep = if place.is_empty() { "" } else { "  " };
        let max = usize::try_from(room - n(&place) - n(sep)).unwrap_or(0);
        label = format!("{live}{place}{sep}{}", limit_line(&what, max, c.ascii));
    }
    let laid = new_list_row(&ListRow {
        mark: &mark,
        label: &label,
        age: &age,
        width: c.width,
        ascii: c.ascii,
        trailing_space: true,
        dim_age: true,
        ..ListRow::default()
    });
    let row = tone_row(&laid, &mark, &live, &place);
    if row_index == c.index {
        format!("{}{row}{}", theme::SELECT_OPEN, theme::SELECT_CLOSE)
    } else if i32::try_from(row_index).is_ok_and(|r| r == c.hover_row) {
        theme::add_hover_span(&row)
    } else {
        row
    }
}

/// The laid-out row with its markers: the live dot green, the place (project, worktree) dim.
/// Added after layout, which strips control characters from a label.
fn tone_row(laid: &str, mark: &str, live: &str, place: &str) -> String {
    let Some(rest) = laid.strip_prefix(mark) else {
        return laid.to_owned();
    };
    let mut out = mark.to_owned();
    let rest = match rest.strip_prefix(live).filter(|_| !live.is_empty()) {
        Some(after) => {
            out.push(theme::TONE_LIVE);
            out.push_str(live.trim_end());
            out.push(theme::TONE_CLOSE);
            out.push(' ');
            after
        }
        None => rest,
    };
    match rest.strip_prefix(place).filter(|_| !place.is_empty()) {
        Some(after) => {
            out.push(theme::DIM_OPEN);
            out.push_str(place);
            out.push(theme::DIM_CLOSE);
            out.push_str(after);
        }
        None => out.push_str(rest),
    }
    out
}

fn picker_footer(scope: PickerScope, g: &Glyphs, width: usize, plain: bool) -> Footer {
    let mut hints = vec![
        Hint::plain("w/s", "move"),
        Hint::ch("/", "filter", '/'),
        Hint::key("enter", "open", "Enter"),
        Hint::ch("f", "fork", 'f'),
    ];
    match scope {
        PickerScope::Project => hints.push(Hint::key("tab", "all projects", "Tab")),
        PickerScope::All => hints.push(Hint::key("tab", "this project", "Tab")),
        PickerScope::None => {}
    }
    hints.push(Hint::key("esc", "back", "Escape"));
    new_hint_footer(&hints, g, width, plain)
}

fn picker_title(v: &PickerView<'_>, shown: usize, hidden: usize, headless: usize, h: char) -> String {
    let mut parts = vec!["resume".to_owned()];
    if v.scope == PickerScope::Project {
        parts.push(if v.project_name.is_empty() {
            "this project".to_owned()
        } else {
            v.project_name.to_owned()
        });
    }
    parts.push(format!("{shown} sessions"));
    if hidden > 0 {
        parts.push(format!("{hidden} empty hidden"));
    }
    if headless > 0 {
        parts.push(format!("{headless} headless hidden"));
    }
    if !v.filter.is_empty() {
        parts.push(format!("filter: {}", clean_text(v.filter)));
    }
    parts.join(&format!(" {h} "))
}

struct Built {
    lines: Vec<String>,
    footer: Footer,
    rows: Vec<(i32, Band)>,
    map: RowMap,
    /// The selected row (absolute index): never hover-banded.
    selected: i32,
}

/// `Get-PickerFrame`, memoised: a frame differing only in its hover repaints those lines.
pub fn picker_frame(v: &PickerView<'_>, memo: &mut Memo) -> Frame {
    if v.width < MIN_WIDTH || v.height < MIN_HEIGHT {
        return Frame {
            lines: too_small_frame(v.width, v.height, v.style.ascii),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
        };
    }
    let g = glyphs(v.style.ascii);
    let key = memo_key(v);
    if let Some(f) = memo.hit(key, v) {
        return f;
    }
    let fw = frame_width(v.width);
    let b = build(v, &g, fw);
    let light = FooterLight {
        hover: footer_hover(&b.footer, v.hover),
        selected: Vec::new(),
    };
    let mut map = b.map;
    let paint = painter();
    let lines = complete_frame(&b.lines, &b.footer, fw, v.style, &paint, &light, &mut map);
    let mut unpainted: Vec<String> = b
        .lines
        .iter()
        .map(|l| l.replace([theme::HOVER_OPEN, theme::HOVER_CLOSE], ""))
        .collect();
    let footer_index = unpainted.len();
    unpainted.extend(
        b.footer
            .lines
            .iter()
            .map(|fl| limit_line(&theme::add_dim_span_color(&fl.text, false), fw, v.style.ascii)),
    );
    *memo = Memo {
        key,
        unpainted,
        painted: lines.clone(),
        hover_row: v.hover_row,
        hover: v.hover,
        rows: b.rows,
        footer: Some(b.footer),
        footer_index,
        width: fw,
        map: map.clone(),
        selected: b.selected,
    };
    Frame { lines, map }
}

fn empty_message(filter: &str, hidden: usize, headless: usize) -> String {
    if !filter.is_empty() {
        "  nothing matches that filter".to_owned()
    } else if headless > 0 {
        format!(
            "  all {} sessions here are empty or headless - nothing to resume",
            hidden + headless
        )
    } else if hidden > 0 {
        format!("  all {hidden} sessions here are empty - nothing to resume")
    } else {
        "  no sessions found".to_owned()
    }
}

/// The widest list pane: past it a wide terminal's room goes to the preview, where text wraps.
const MAX_LIST_WIDTH: usize = 72;

/// The box's inner rows at this height: the terminal less the border, the footer and the
/// headroom row.
fn body_rows(height: usize, footer: &Footer) -> usize {
    height.saturating_sub(3 + footer.lines.len()).max(3)
}

/// (preview rows, list rows): the wide layout lists in every body row; the narrow one keeps a
/// preview of 6 rows (2 below 30) under the list.
fn split_rows(wide: bool, height: usize, body_rows: usize) -> (usize, usize) {
    if wide {
        (0, body_rows)
    } else {
        let p = if height < SHORT_PICKER_HEIGHT { 2 } else { 6 };
        (p, body_rows.saturating_sub(p).max(1))
    }
}

/// How many sessions a frame of this size lists: the picker loads pages until they fill it.
pub fn list_capacity(width: usize, height: usize, scope: PickerScope, style: Style) -> usize {
    let g = glyphs(style.ascii);
    let footer = picker_footer(scope, &g, frame_width(width), !style.color);
    split_rows(width >= TWO_PANE_WIDTH, height, body_rows(height, &footer)).1
}

/// (list pane, detail pane) widths: 40 % for the list at >= 100 columns (at most
/// `MAX_LIST_WIDTH`), else the whole frame.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a frame width; [int] is banker's rounding"
)]
fn pane_widths(fw: usize, wide: bool) -> (usize, usize) {
    let inner = fw.saturating_sub(2);
    let left = if wide {
        28.max((inner as f64 * 0.4).round_ties_even() as usize).min(MAX_LIST_WIDTH)
    } else {
        inner
    };
    (left, inner.saturating_sub(left + 1))
}

fn build(v: &PickerView<'_>, g: &Glyphs, fw: usize) -> Built {
    let ascii = v.style.ascii;
    let resumable_rows: Vec<&Session> = v.sessions.iter().copied().filter(|s| resumable(s)).collect();
    let headless = v.sessions.iter().filter(|s| s.headless).count();
    let hidden = v.sessions.len().saturating_sub(resumable_rows.len() + headless);
    let items: Vec<&Session> = resumable_rows.into_iter().filter(|s| matches(s, v.filter)).collect();
    let index = v.index.min(items.len().saturating_sub(1));
    let h = g.h;
    let title = picker_title(v, items.len(), hidden, headless, h);
    let footer = picker_footer(v.scope, g, fw, !v.style.color);
    let body_rows = body_rows(v.height, &footer);
    if items.is_empty() {
        let mut body = vec![String::new(), empty_message(v.filter, hidden, headless)];
        body.resize(body_rows, String::new());
        return Built {
            lines: new_box(&body, fw, &title, g, ascii),
            footer,
            rows: Vec::new(),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
            selected: -1,
        };
    }
    let wide = v.width >= TWO_PANE_WIDTH;
    let (left_width, right_width) = pane_widths(fw, wide);
    let ctx = RowCtx {
        index,
        hover_row: v.hover_row,
        scope: v.scope,
        clock: v.clock,
        g,
        width: left_width,
        ascii,
    };
    let s = items.get(index).copied().unwrap_or(items[0]);
    let mut list = Vec::new();
    let mut bands = Vec::new();
    let (preview_rows, list_rows) = split_rows(wide, v.height, body_rows);
    let (start, visible) = viewport(items.len(), index, list_rows);
    for (i, it) in items.iter().enumerate().skip(start).take(visible) {
        let row = session_row(it, i, &ctx);
        bands.push((i, i - start, row_band_length(&row, left_width)));
        list.push(row);
    }
    let pv = Preview {
        s,
        g,
        ascii,
        offset: v.clock.offset_minutes,
    };
    // AC27: the box always fills the terminal; the panes pad to the full body height.
    list.resize(list_rows, String::new());
    let body = if wide {
        let detail: Vec<String> = pv
            .card(right_width.saturating_sub(2), body_rows, v.scope, v.chat)
            .into_iter()
            .map(|l| format!(" {l}"))
            .collect();
        join_panes(&list, &detail, left_width, right_width, g, ascii)
    } else {
        let mut body = list;
        body.extend(
            pv.card(fw.saturating_sub(6), preview_rows, v.scope, v.chat)
                .into_iter()
                .map(|l| format!("  {l}")),
        );
        body.resize(body_rows, String::new());
        body
    };
    let lines = new_box(&body, fw, &title, g, ascii);
    let first_row_y = lines.len() - body.len() - 1;
    let to_i = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    Built {
        lines,
        footer,
        rows: bands
            .into_iter()
            .map(|(i, y, len)| {
                (
                    to_i(i),
                    Band {
                        y: first_row_y + y,
                        start: 1,
                        len,
                    },
                )
            })
            .collect(),
        map: RowMap {
            list: Some(ListMap {
                first_row_y: to_i(first_row_y),
                row_count: to_i(visible),
                start: to_i(start),
                row_ys: Vec::new(),
            }),
            ..RowMap::default()
        },
        selected: to_i(index),
    }
}

#[cfg(test)]
mod tests;
