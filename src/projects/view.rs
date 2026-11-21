//! The project frame and its viewport: where the session runs and what it does there.
//! Pure: every input, the clock included, is a parameter.

use std::sync::LazyLock;

use regex::Regex;

use super::Project;
use super::paths::project_key;
use super::select::{Row, RowKind, filter_words, project_count, project_rows};
use crate::cockpit::{self, Focus, Panel};
use crate::frame::{
    self, ActionHit, CellSpan, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RadioRow, RadioSpec, RowMap, Style,
    TWO_PANE_WIDTH, new_hint_footer, new_radio_row,
};
use crate::launch::{LAUNCH_MODES, launch_mode, step_option};
use crate::layout::{display_width, limit_line, limit_path, viewport};
use crate::list::{Columns, ListRow, add_picker_color, new_list_row};
use crate::theme::{self, Glyphs, glyphs};

mod marks;
#[cfg(test)]
use marks::MATCH_ON;
use marks::{add_match_color, dim_after_mark, dim_char_at, mark_matches};

/// The action field's values: the launch-mode table's names, in its order.
pub(crate) fn project_actions() -> Vec<String> {
    LAUNCH_MODES.iter().map(|m| m.name.to_owned()).collect()
}

/// Steps the project action; delta 0 is the canonicaliser (an unknown value is the first).
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

/// Now, N min, N h, N d (rounded like `{0:N0}`).
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

/// Control and format characters become spaces, runs of space collapse.
pub(crate) fn clean_text(text: &str) -> String {
    let t = CTL_OR_FORMAT.replace_all(text, " ");
    SPACES.replace_all(&t, " ").trim().to_owned()
}

/// The window over the rows, separator lines included, centred on the
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
    /// Project keys drawn dimmed and marked missing: the pins whose folder is gone.
    pub dimmed: &'a [String],
    /// Project keys of the pinned projects: a dim pin mark before the name.
    pub pinned: &'a [String],
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
        Hint::plain(frame::switch_keys(&glyphs(v.style.ascii)), "action").kept(),
        Hint::key("enter", "run", "Enter"),
    ];
    h.extend(
        LAUNCH_MODES
            .iter()
            .filter_map(|m| m.hotkey.map(|k| Hint::ch(&k.to_string(), m.name, k))),
    );
    h.push(Hint::ch("/", "filter", '/'));
    h.push(Hint::ch("p", "pin", 'p'));
    h.push(Hint::ch("h", "hide", 'h'));
    h.push(Hint::ch("v", if v.show_hidden { "back" } else { "hidden" }, 'v'));
    h.push(Hint::key("esc", if v.filter.is_empty() { "back" } else { "clear" }, "Escape"));
    h
}

fn title(v: &ProjectView<'_>, g: &Glyphs) -> String {
    let (known, hidden) = project_count(v.projects, v.filter, v.cwd);
    let d = g.dot;
    let filter =
        (!v.filter.is_empty() || v.typing).then(|| format!(" {d} filter: {}{}", clean_text(v.filter), if v.typing { "_" } else { "" }));
    // A rejected pick says so once, in place of the counts: they would push it out of the border.
    if !v.notice.is_empty() {
        return format!("project{} {d} {}", filter.unwrap_or_default(), clean_text(v.notice));
    }
    let mut t = format!("project {d} {known} known");
    let worktrees = v.projects.iter().filter(|p| p.worktree.is_some()).count();
    for (n, what) in [(v.pinned.len(), "pinned"), (hidden, "hidden"), (worktrees, "worktrees")] {
        if n > 0 {
            t = format!("{t} {d} {what} {n}");
        }
    }
    if !v.marked.is_empty() {
        t = format!("{t} {d} {} hidden listed", v.marked.len());
    }
    t + &filter.unwrap_or_default()
}

/// A project or cwd row's label: the name with its pin, worktree and hidden or missing marks.
fn label_text(r: &Row, v: &ProjectView<'_>, g: &Glyphs) -> String {
    let key = project_key(&r.path);
    let project = r.kind == RowKind::Project;
    let n = if project && v.marked.contains(&key) {
        format!("{} (hidden)", r.name)
    } else if project && v.dimmed.contains(&key) {
        format!("{} (missing)", r.name)
    } else {
        r.name.clone()
    };
    let n = if r.worktree { format!("{} {n}", g.worktree) } else { n };
    if project && v.pinned.contains(&key) {
        format!("{} {n}", g.up)
    } else {
        n
    }
}

fn age_text(r: &Row, v: &ProjectView<'_>) -> String {
    r.last_activity_ms.map(|t| relative_age(t, v.now_ms)).unwrap_or_default()
}

/// The compact columns of the visible rows: the longest name (at most 40 % of the inner width),
/// the longest path cut to what is left of `inner`, the widest age.
fn list_columns(rows: &[Row], inner: usize, v: &ProjectView<'_>, g: &Glyphs) -> Columns {
    let listed = || rows.iter().filter(|r| r.kind != RowKind::Path);
    let widest = |w: &dyn Fn(&Row) -> usize| listed().map(w).max().unwrap_or(0);
    let label = widest(&|r| display_width(&theme::strip_ctl(&label_text(r, v, g)))).min(inner * 2 / 5);
    let age = widest(&|r| display_width(&age_text(r, v)));
    let room = inner.saturating_sub(3 + label + 2 + if age > 0 { 1 + age } else { 0 });
    let tail = widest(&|r| display_width(&theme::strip_ctl(&r.path))).min(room);
    Columns { label, tail, age }
}

/// One list row's text: project, cwd, or the free-path row. `cols`: the wide frame's columns.
fn row_text(r: &Row, mark: &str, hovered: bool, inner: usize, cols: Option<Columns>, v: &ProjectView<'_>, g: &Glyphs) -> String {
    let ascii = v.style.ascii;
    let key = project_key(&r.path);
    let missing = r.kind == RowKind::Project && v.dimmed.contains(&key);
    let pinned = r.kind == RowKind::Project && v.pinned.contains(&key);
    let age = age_text(r, v);
    match r.kind {
        RowKind::Project | RowKind::Cwd => {
            let label = label_text(r, v, g);
            let base = ListRow {
                mark,
                label: &label,
                width: inner,
                ascii,
                hover: hovered,
                ..ListRow::default()
            };
            let row = if cols.is_some() {
                new_list_row(&ListRow {
                    tail: &r.path,
                    age: &age,
                    dim_tail: true,
                    dim_age: true,
                    path_tail: true,
                    columns: cols,
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
            let row = if missing {
                dim_after_mark(&row, mark.chars().count())
            } else if pinned {
                dim_char_at(&row, mark.chars().count())
            } else {
                row
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
    cols: Option<Columns>,
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
        body.push(row_text(r, &mark, hover_row == Some(i), w.inner, w.cols, v, g));
        if r.kind == RowKind::Cwd && i + 1 < end && rows.get(i + 1).is_some_and(|n| n.kind != RowKind::Path) {
            body.push(String::new());
        }
    }
    (body, row_ys)
}

/// The current directory leads, the registry follows newest first, the
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
    let wide = v.width >= cockpit::TWO_COLUMNS;
    let columns = v.width >= TWO_PANE_WIDTH;
    let lw = if wide { cockpit::LEFT_WIDTH } else { fw };
    let page_rows = v.height.saturating_sub(1 + footer.lines.len()).max(8);
    let index = v.index.min(rows.len().saturating_sub(1));
    // AC27: the list takes every row of its panel, so the panel fills a tall terminal. Stacked, the
    // path of the cursor's row and the action field close the panel.
    let list_lines = page_rows.saturating_sub(if wide { 2 } else { 4 }).max(3);
    let (start, visible) = project_viewport(&rows, index, list_lines);
    // Two cells free before the right border.
    let inner = lw.saturating_sub(4);
    let shown = rows.get(start..start + visible).unwrap_or_default();
    let w = Window {
        index,
        start,
        visible,
        inner,
        cols: columns.then(|| list_columns(shown, inner, v, &g)),
    };
    let (mut body, row_ys) = list_body(&rows, &w, v, &g);
    let selected = row_ys.get(index.saturating_sub(start)).copied();
    let painter = |l: &str, enabled: bool| add_match_color(&add_picker_color(l, enabled, &g), enabled);
    let radio_inner = if wide { fw - lw - 2 } else { inner };
    let radio = action_radio(v.action, v.hover_value, radio_inner, &g, !wide);
    let mut action = None;
    if !wide {
        body.resize(list_lines, String::new());
        let path = theme::strip_ctl(rows.get(index).map_or("", |r| r.path.as_str()));
        body.push(if path.is_empty() {
            String::new()
        } else {
            let line = format!("   {}", cockpit::dim(&limit_path(&path, inner.saturating_sub(4), v.style.ascii)));
            mark_matches(&line, &filter_words(v.filter), 3, usize::MAX)
        });
        action = Some(action_hit(&radio, i32::try_from(1 + body.len()).unwrap_or(0), 0));
        body.push(radio.text.clone());
    }
    let list = Panel {
        title: cockpit::identity(&g),
        page: title(v, &g),
        right: String::new(),
        focus: Focus::Focused,
        lines: body,
        width: lw,
        height: page_rows,
        painter: &painter,
        selected,
    };
    let mut lines = cockpit::render(&list, v.style);
    if wide {
        let rw = fw - lw;
        let sel = rows.get(index);
        let detail_h = 6;
        let mut detail = Panel::plain(
            &format!("selected {} {}", g.dot, sel.map_or(String::new(), |r| clean_text(&r.name))),
            selected_lines(sel, v, rw - 2),
            rw,
            detail_h,
        );
        detail.painter = &painter;
        let mut right = cockpit::render(&detail, v.style);
        let mut act = Panel::plain("action", vec![radio.text.clone()], rw, page_rows - detail_h);
        act.painter = &painter;
        right.extend(cockpit::render(&act, v.style));
        action = Some(action_hit(&radio, i32::try_from(detail_h + 1).unwrap_or(0), lw));
        lines = cockpit::beside(lines, lw, &right);
    }
    let y = |n: usize| i32::try_from(1 + n).unwrap_or(i32::MAX);
    let mut map = RowMap {
        list: Some(ListMap {
            first_row_y: y(0),
            row_count: i32::try_from(visible).unwrap_or(0),
            start: i32::try_from(start).unwrap_or(0),
            row_ys: row_ys.iter().map(|r| y(*r)).collect(),
        }),
        action,
        ..RowMap::default()
    };
    let light = FooterLight {
        hover: frame::footer_hover(&footer, v.hover),
        selected: action_light(v.action),
    };
    let lines = frame::append_footer(lines, &footer, fw, v.style, &light, &mut map);
    Frame { lines, map }
}

/// The cursor's project beside the list: its path, how long ago it ran, its worktree.
fn selected_lines(r: Option<&Row>, v: &ProjectView<'_>, inner: usize) -> Vec<String> {
    let Some(r) = r else { return Vec::new() };
    let mut out = vec![format!(
        "  {}",
        cockpit::dim(&limit_path(&theme::strip_ctl(&r.path), inner.saturating_sub(4), v.style.ascii))
    )];
    let age = age_text(r, v);
    if !age.is_empty() {
        out.push(format!("  {}  {age}", cockpit::dim("last session")));
    }
    if r.worktree {
        out.push(format!("  {}", cockpit::dim("a worktree")));
    }
    out
}

/// The action field as a radio row inside a box of `inner` cells; unlabelled in its own panel,
/// whose title names it.
pub(crate) fn action_radio(action: &str, hover: &str, inner: usize, g: &Glyphs, labelled: bool) -> RadioRow {
    let values = project_actions();
    let current = step_project_action(action, 0);
    new_radio_row(
        &RadioSpec {
            prefix: if labelled { "   " } else { "  " },
            label: if labelled { "action" } else { "" },
            values: &values,
            current: &current,
            texts: &values,
            label_width: if labelled { 8 } else { 0 },
            max_width: inner,
            hover,
            compact: false,
        },
        g,
    )
}

/// The radio row's click cells at frame line `y`, inside a panel whose left border is column `x0`.
pub(crate) fn action_hit(radio: &RadioRow, y: i32, x0: usize) -> ActionHit {
    let x0 = i32::try_from(x0).unwrap_or(0);
    ActionHit {
        y,
        cells: radio
            .cells
            .iter()
            .map(|c| CellSpan {
                start: c.start + 1 + x0,
                end: c.end + 1 + x0,
                value: c.value.clone(),
            })
            .collect(),
    }
}

/// The footer button of the selected action, lit.
pub(crate) fn action_light(action: &str) -> Vec<(String, Option<char>)> {
    launch_mode(&step_project_action(action, 0))
        .and_then(|m| m.hotkey)
        .map(|k| (String::new(), Some(k)))
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests;
