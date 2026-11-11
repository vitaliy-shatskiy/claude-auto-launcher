//! `Get-ProjectFrame` and `Get-ProjectViewport`: where the session runs and what it does there.
//! Pure: every input, the clock included, is a parameter.

use std::sync::LazyLock;

use regex::Regex;

use super::Project;
use super::paths::project_key;
use super::select::{Row, RowKind, filter_words, project_count, project_rows};
use crate::frame::{
    self, ActionHit, CellSpan, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RadioSpec, RowMap, Style, TWO_PANE_WIDTH,
    new_hint_footer, new_radio_row,
};
use crate::launch::{LAUNCH_MODES, launch_mode, step_option};
use crate::layout::{display_width, limit_line, limit_path, new_box, viewport};
use crate::list::{ListRow, add_picker_color, new_list_row};
use crate::theme::{self, Glyphs, glyphs};

/// The action field's values: the launch-mode table's names, in its order.
pub(crate) fn project_actions() -> Vec<String> {
    LAUNCH_MODES.iter().map(|m| m.name.to_owned()).collect()
}

/// `Step-ProjectAction`; delta 0 is the canonicaliser (an unknown value is the first).
pub(crate) fn step_project_action(action: &str, delta: i64) -> String {
    step_option(&project_actions(), action, delta)
}

/// Thousands-grouped whole number, the invariant `{0:N0}`.
fn group(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "ages in minutes, hours or days fit easily"
)]
fn rounded(ms: i64, unit_ms: f64) -> i64 {
    (ms as f64 / unit_ms).round() as i64
}

/// `Format-RelativeAge`: now, N min, N h, N d (rounded like `{0:N0}`).
pub(crate) fn relative_age(from_ms: i64, now_ms: i64) -> String {
    let age = now_ms.saturating_sub(from_ms);
    if age < 60_000 {
        "now".into()
    } else if age < 3_600_000 {
        format!("{} min", group(rounded(age, 60_000.0)))
    } else if age < 86_400_000 {
        format!("{} h", group(rounded(age, 3_600_000.0)))
    } else {
        format!("{} d", group(rounded(age, 86_400_000.0)))
    }
}

static CTL_OR_FORMAT: LazyLock<Regex> = LazyLock::new(|| theme::re(r"[\p{Cc}\p{Cf}]"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\s+"));

/// `Get-CleanTranscriptText`: control and format characters become spaces, runs of space collapse.
pub(crate) fn clean_text(text: &str) -> String {
    let t = CTL_OR_FORMAT.replace_all(text, " ");
    SPACES.replace_all(&t, " ").trim().to_owned()
}

/// `Get-ProjectViewport`: the window over the rows, separator lines included, centred on the
/// cursor's line. A window never opens on a gap; one closing on a gap ends a line earlier.
/// Returns (first row, visible rows).
pub(crate) fn project_viewport(rows: &[Row], index: usize, lines: usize) -> (usize, usize) {
    let count = rows.len();
    if count == 0 {
        return (0, 0);
    }
    let mut line_row: Vec<Option<usize>> = Vec::new();
    let mut row_line = vec![0usize; count];
    for (i, r) in rows.iter().enumerate() {
        if r.kind == RowKind::Path && i > 0 {
            line_row.push(None);
        }
        row_line[i] = line_row.len();
        line_row.push(Some(i));
        if r.kind == RowKind::Cwd && rows.get(i + 1).is_some_and(|n| n.kind != RowKind::Path) {
            line_row.push(None);
        }
    }
    let index = index.min(count - 1);
    let (start, visible) = viewport(line_row.len(), row_line[index], lines.max(1));
    let mut first = start;
    let mut last = start + visible - 1;
    if line_row.get(first).copied().flatten().is_none() {
        first += 1;
        last = (last + 1).min(line_row.len() - 1);
    }
    while last > first && line_row.get(last).copied().flatten().is_none() {
        last -= 1;
    }
    let a = line_row.get(first).copied().flatten().unwrap_or(0);
    let b = line_row.get(last).copied().flatten().unwrap_or(a);
    (a, b - a + 1)
}

/// Everything one project frame is drawn from.
#[derive(Debug, Clone)]
pub(crate) struct ProjectView<'a> {
    pub projects: &'a [Project],
    /// Project keys drawn with a hidden mark: the show-hidden view's hidden projects.
    pub marked: &'a [String],
    pub index: usize,
    pub filter: &'a str,
    pub typing: bool,
    /// The `v` view: every project listed, the hidden ones marked.
    pub show_hidden: bool,
    pub cwd: &'a str,
    pub width: usize,
    pub height: usize,
    pub now_ms: i64,
    pub style: Style,
    /// Footer button under the mouse (-1 none).
    pub hover: i32,
    /// A rejected pick, shown once in the title.
    pub notice: &'a str,
    pub action: &'a str,
    /// List index under the mouse (-1 none); paints only.
    pub hover_row: i32,
    /// Action value under the mouse ('' none); paints only.
    pub hover_value: &'a str,
}

/// The footer follows the state: an open filter names only what works while typing (every
/// button a named key, so a click on one is never typed into the filter).
fn footer_hints(v: &ProjectView<'_>) -> Vec<Hint> {
    if v.typing {
        return vec![
            Hint::plain("up/down", "move").kept(),
            Hint::plain("left/right", "action").kept(),
            Hint::key("enter", "run", "Enter"),
            Hint::key("esc", "clear", "Escape"),
        ];
    }
    let mut h = vec![
        Hint::plain("w/s", "move").kept(),
        Hint::plain("a/d", "action").kept(),
        Hint::key("enter", "run", "Enter"),
    ];
    h.extend(
        LAUNCH_MODES
            .iter()
            .filter_map(|m| m.hotkey.map(|k| Hint::ch(&k.to_string(), m.name, k))),
    );
    h.push(Hint::ch("/", "filter", '/'));
    h.push(Hint::ch("h", "hide", 'h'));
    h.push(Hint::ch("v", if v.show_hidden { "back" } else { "hidden" }, 'v'));
    h.push(Hint::key("esc", if v.filter.is_empty() { "back" } else { "clear" }, "Escape"));
    h
}

fn title(v: &ProjectView<'_>, g: &Glyphs) -> String {
    let (known, hidden) = project_count(v.projects, v.filter, v.cwd);
    let mut t = format!("project {} {known} known", g.h);
    if hidden > 0 {
        t = format!("{t} {} {hidden} hidden", g.h);
    }
    if !v.marked.is_empty() {
        t = format!("{t} {} {} hidden listed", g.h, v.marked.len());
    }
    if !v.filter.is_empty() || v.typing {
        t = format!("{t} {} filter: {}", g.h, clean_text(v.filter));
        if v.typing {
            t.push('_');
        }
    }
    if !v.notice.is_empty() {
        t = format!("{t} {} {}", g.h, clean_text(v.notice));
    }
    t
}

/// Filter-match markers, resolved after layout like the dim ones (C0, zero cells wide).
const MATCH_OPEN: char = '\u{6}';
const MATCH_CLOSE: char = '\u{e}';
/// Underline and the accent, closed without a Reset so a dim tail or a hover band stays on.
const MATCH_ON: &str = "\x1b[4m\x1b[38;5;209m";
const MATCH_OFF: &str = "\x1b[24m\x1b[39m";

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Marks every case-insensitive occurrence of a filter word among the visible characters
/// `from..to` of a finished row (control characters are markers and skipped).
fn mark_matches(row: &str, words: &[String], from: usize, to: usize) -> String {
    let chars: Vec<char> = row.chars().collect();
    let shown: Vec<char> = chars.iter().copied().filter(|c| !c.is_control()).map(fold).collect();
    let to = to.min(shown.len());
    let mut hit = vec![false; shown.len()];
    for w in words {
        let w: Vec<char> = w.chars().map(fold).collect();
        if w.is_empty() || from + w.len() > to {
            continue;
        }
        for s in from..=to - w.len() {
            if shown.get(s..s + w.len()) == Some(w.as_slice()) {
                hit.iter_mut().skip(s).take(w.len()).for_each(|h| *h = true);
            }
        }
    }
    if !hit.contains(&true) {
        return row.to_owned();
    }
    let mut out = String::new();
    let (mut at, mut open) = (0, false);
    for c in chars {
        if !c.is_control() {
            let h = hit.get(at).copied().unwrap_or(false);
            if h != open {
                out.push(if h { MATCH_OPEN } else { MATCH_CLOSE });
                open = h;
            }
            at += 1;
        }
        out.push(c);
    }
    if open {
        out.push(MATCH_CLOSE);
    }
    out
}

/// The match markers to colour, or to nothing with colour off.
fn add_match_color(line: &str, enabled: bool) -> String {
    if enabled {
        line.replace(MATCH_OPEN, MATCH_ON).replace(MATCH_CLOSE, MATCH_OFF)
    } else {
        line.replace([MATCH_OPEN, MATCH_CLOSE], "")
    }
}

/// One list row's text: project, cwd, or the free-path row.
fn row_text(r: &Row, mark: &str, hovered: bool, inner: usize, wide: bool, v: &ProjectView<'_>, g: &Glyphs) -> String {
    let ascii = v.style.ascii;
    let marked = r.kind == RowKind::Project && v.marked.contains(&project_key(&r.path));
    let named = |n: &str| {
        let n = if marked { format!("{n} (hidden)") } else { n.to_owned() };
        if r.worktree { format!("{} {n}", g.worktree) } else { n }
    };
    let age = r.last_activity_ms.map(|t| relative_age(t, v.now_ms)).unwrap_or_default();
    match r.kind {
        RowKind::Project | RowKind::Cwd => {
            let label = named(&r.name);
            let tail = if wide { r.path.as_str() } else { "" };
            let base = ListRow {
                mark,
                label: &label,
                width: inner,
                ascii,
                hover: hovered,
                ..ListRow::default()
            };
            let row = if r.kind == RowKind::Cwd && age.is_empty() {
                new_list_row(&ListRow {
                    tail,
                    dim_tail: true,
                    path_tail: true,
                    ..base
                })
            } else if wide {
                new_list_row(&ListRow {
                    tail,
                    age: &age,
                    dim_tail: true,
                    dim_age: true,
                    path_tail: true,
                    ..base
                })
            } else {
                new_list_row(&ListRow {
                    age: &age,
                    dim_age: true,
                    age_gutter: true,
                    ..base
                })
            };
            // The mark and the age are never matched: only name and path are.
            let shown: String = row.chars().filter(|c| !c.is_control()).collect();
            let to = if age.is_empty() { None } else { shown.rfind(&age) }
                .map_or(usize::MAX, |b| shown.get(..b).map_or(0, |h| h.chars().count()));
            mark_matches(&row, &filter_words(v.filter), mark.chars().count(), to)
        }
        RowKind::Path => {
            let name = limit_line(&theme::strip_ctl(&r.name), inner.saturating_sub(mark.chars().count() + 3), ascii);
            let row = format!("{mark}{} {name}", g.bullet);
            if hovered {
                theme::add_hover_span(&format!("{row}{}", " ".repeat(inner.saturating_sub(display_width(&row)))))
            } else {
                row
            }
        }
    }
}

/// Where the list sits in one frame.
struct Window {
    index: usize,
    start: usize,
    visible: usize,
    inner: usize,
    wide: bool,
}

/// The visible rows and the body line of each. Two separator rules, mirrored by
/// `project_viewport`: a blank line above the free-path row unless it opens the window, and one
/// under the cwd row unless it closes the window or the free-path row follows it.
fn list_body(rows: &[Row], w: &Window, v: &ProjectView<'_>, g: &Glyphs) -> (Vec<String>, Vec<usize>) {
    let hover_row = usize::try_from(v.hover_row).ok();
    let end = w.start + w.visible;
    let mut body: Vec<String> = Vec::new();
    let mut row_ys: Vec<usize> = Vec::new();
    for i in w.start..end {
        let Some(r) = rows.get(i) else { break };
        if r.kind == RowKind::Path && !body.is_empty() {
            body.push(String::new());
        }
        let mark = if i == w.index {
            format!(" {} ", g.cursor)
        } else {
            "   ".to_owned()
        };
        row_ys.push(body.len());
        body.push(row_text(r, &mark, hover_row == Some(i), w.inner, w.wide, v, g));
        if r.kind == RowKind::Cwd && i + 1 < end && rows.get(i + 1).is_some_and(|n| n.kind != RowKind::Path) {
            body.push(String::new());
        }
    }
    (body, row_ys)
}

/// `Get-ProjectFrame`. The current directory leads, the registry follows newest first, the
/// free-path row closes the list; the action field is a radio row under it.
pub(crate) fn project_frame(v: &ProjectView<'_>) -> Frame {
    if v.width < MIN_WIDTH || v.height < MIN_HEIGHT {
        return Frame {
            lines: frame::too_small_frame(v.width, v.height, v.style.ascii),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
        };
    }
    let g = glyphs(v.style.ascii);
    let fw = frame::frame_width(v.width);
    let rows = project_rows(v.projects, v.filter, v.cwd);
    let footer = new_hint_footer(&footer_hints(v), &g, fw, !v.style.color);
    let wide = v.width >= TWO_PANE_WIDTH;
    let detail = usize::from(!wide);
    let body_rows = v.height.saturating_sub(6 + detail + footer.lines.len()).max(3);
    let index = v.index.min(rows.len().saturating_sub(1));
    let list_lines = viewport(rows.len(), index, body_rows).1 + 2;
    let (start, visible) = project_viewport(&rows, index, list_lines);
    let inner = fw.saturating_sub(2);
    let w = Window {
        index,
        start,
        visible,
        inner,
        wide,
    };
    let (mut body, row_ys) = list_body(&rows, &w, v, &g);
    while body.len() < list_lines {
        body.push(String::new());
    }
    if !wide {
        let path = theme::strip_ctl(rows.get(index).map_or("", |r| r.path.as_str()));
        body.push(if path.is_empty() {
            String::new()
        } else {
            let line = format!(
                "   {}{}{}",
                theme::DIM_OPEN,
                limit_path(&path, inner.saturating_sub(4), v.style.ascii),
                theme::DIM_CLOSE
            );
            mark_matches(&line, &filter_words(v.filter), 3, usize::MAX)
        });
    }

    let values = project_actions();
    let current = step_project_action(v.action, 0);
    let radio = new_radio_row(
        &RadioSpec {
            prefix: "   ",
            label: "action",
            values: &values,
            current: &current,
            texts: &values,
            label_width: 8,
            max_width: inner,
            hover: v.hover_value,
            compact: false,
        },
        &g,
    );
    let action_index = body.len();
    body.push(radio.text);

    let lines = new_box(&body, fw, &title(v, &g), &g, v.style.ascii);
    let first_row_y = lines.len() - body.len() - 1;
    let y = |n: usize| i32::try_from(first_row_y + n).unwrap_or(i32::MAX);
    let mut map = RowMap {
        list: Some(ListMap {
            first_row_y: y(0),
            row_count: i32::try_from(visible).unwrap_or(0),
            start: i32::try_from(start).unwrap_or(0),
            row_ys: row_ys.iter().map(|r| y(*r)).collect(),
        }),
        action: Some(ActionHit {
            y: y(action_index),
            cells: radio
                .cells
                .iter()
                .map(|c| CellSpan {
                    start: c.start + 1,
                    end: c.end + 1,
                    value: c.value.clone(),
                })
                .collect(),
        }),
        ..RowMap::default()
    };
    let selected = launch_mode(&current)
        .and_then(|m| m.hotkey)
        .map(|k| (String::new(), Some(k)))
        .into_iter()
        .collect();
    let light = FooterLight {
        hover: frame::footer_hover(&footer, v.hover),
        selected,
    };
    let painter = |l: &str, enabled: bool| add_match_color(&add_picker_color(l, enabled, &g), enabled);
    let lines = frame::complete_frame(&lines, &footer, fw, v.style, &painter, &light, &mut map);
    Frame { lines, map }
}

#[cfg(test)]
mod tests;
