//! The project frame and its viewport: where the session runs and what it does there.
//! Pure: every input, the clock included, is a parameter.

use std::sync::LazyLock;

use regex::Regex;

use super::Project;
use super::detail::Detail;
use super::paths::project_key;
use super::select::{Row, RowKind, filter_words, project_count, project_rows};
use crate::cockpit::{self, Focus, Panel};
use crate::frame::{
    self, ActionHit, CellSpan, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RadioRow, RadioSpec, RowHit, RowMap, SESSION_ROW,
    Style, TWO_PANE_WIDTH, new_hint_footer, new_radio_row,
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
    /// The selected row's sessions and branch, once loaded (None = still loading).
    pub detail: Option<&'a Detail>,
    /// On the launch page: which panel has focus and the list panel's width (the launch panel's
    /// above it). `None` = the project page, the list focused at its own width.
    pub home: Option<Home>,
}

/// Which panel of the launch page's workspace has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HomeFocus {
    Launch,
    Projects,
    /// The selected project's sessions, the cursor on the n-th.
    Sessions(usize),
}

/// The list drawn under the launch panel: its focus and its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Home {
    pub focus: HomeFocus,
    pub left_width: usize,
}

/// The footer follows the state: an open filter names only what works while typing (every
/// button a named key, so a click on one is never typed into the filter).
fn footer_hints(v: &ProjectView<'_>) -> Vec<Hint> {
    match v.home.map(|h| h.focus) {
        Some(HomeFocus::Sessions(_)) => {
            // Resume and fork only where the panel lists a session of the selected project.
            let rows = project_rows(v.projects, v.filter, v.cwd);
            let sel = rows.get(v.index.min(rows.len().saturating_sub(1)));
            let listed = v
                .detail
                .is_some_and(|d| !d.sessions.is_empty() && sel.is_some_and(|r| r.kind != RowKind::Path && d.key == project_key(&r.path)));
            let mut h = vec![Hint::plain("w/s", "move").kept()];
            if listed {
                h.push(Hint::key("enter", "resume", "Enter"));
                h.push(Hint::ch("f", "fork", 'f'));
                h.push(Hint::ch("v", "chat", 'v'));
            }
            h.push(Hint::key("tab", "launch", "Tab"));
            h.push(Hint::key("esc", "projects", "Escape"));
            return h;
        }
        Some(HomeFocus::Projects) if !v.typing => {
            let mut h = project_hints(v);
            h.insert(3, Hint::key("tab", "sessions", "Tab"));
            return h;
        }
        Some(HomeFocus::Launch | HomeFocus::Projects) | None => {}
    }
    if v.typing {
        return vec![
            Hint::plain("up/down", "move").kept(),
            Hint::plain("left/right", "action").kept(),
            Hint::key("enter", "run", "Enter"),
            Hint::key("esc", "clear", "Escape"),
        ];
    }
    project_hints(v)
}

/// The list's own keys, the filter closed.
fn project_hints(v: &ProjectView<'_>) -> Vec<Hint> {
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
    // The filter leads: a narrow list panel cuts the title's tail, never what is typed.
    let mut t = format!("project{} {d} {known} known", filter.unwrap_or_default());
    let worktrees = v.projects.iter().filter(|p| p.worktree.is_some()).count();
    for (n, what) in [(v.pinned.len(), "pinned"), (hidden, "hidden"), (worktrees, "worktrees")] {
        if n > 0 {
            t = format!("{t} {d} {what} {n}");
        }
    }
    if !v.marked.is_empty() {
        t = format!("{t} {d} {} hidden listed", v.marked.len());
    }
    t
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

/// The columns of the visible rows: the longest name (at most 40 % of the inner width), the path
/// in what is left of `inner`, the widest age right-aligned at the panel's edge.
fn list_columns(rows: &[Row], inner: usize, v: &ProjectView<'_>, g: &Glyphs) -> Columns {
    let listed = || rows.iter().filter(|r| r.kind != RowKind::Path);
    let widest = |w: &dyn Fn(&Row) -> usize| listed().map(w).max().unwrap_or(0);
    let label = widest(&|r| display_width(&theme::strip_ctl(&label_text(r, v, g)))).min(inner * 2 / 5);
    let age = widest(&|r| display_width(&age_text(r, v)));
    let room = inner.saturating_sub(3 + label + 2 + if age > 0 { 1 + age } else { 0 });
    Columns { label, tail: room, age }
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
    // Beside the launch panel with the launch rows focused, the list stands alone in its column.
    let wide = v.width >= cockpit::TWO_COLUMNS && v.home.is_none_or(|h| h.focus != HomeFocus::Launch);
    // On the launch page the list keeps its name, path and age columns on every focus: a focus
    // move never shifts a row.
    let columns = v.width >= TWO_PANE_WIDTH || v.home.is_some();
    let page_rows = v.height.saturating_sub(1 + footer.lines.len()).max(8);
    let index = v.index.min(rows.len().saturating_sub(1));
    if !wide && matches!(v.home.map(|h| h.focus), Some(HomeFocus::Sessions(_))) {
        return sessions_page(v, rows.get(index), (fw, page_rows), &footer, &g);
    }
    // AC27: the list takes every row of its panel, so the panel fills a tall terminal. The path of
    // the cursor's row (stacked; beside, the selected panel has it) and the action field close it.
    let list_lines = page_rows.saturating_sub(4).max(3);
    let (start, visible) = project_viewport(&rows, index, list_lines);
    let lw = list_panel_width(v, fw, wide);
    let list_focused = v.home.is_none_or(|h| h.focus == HomeFocus::Projects);
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
    let radio = action_radio(v.action, v.hover_value, inner, &g, true);
    body.resize(list_lines, String::new());
    let path = theme::strip_ctl(rows.get(index).map_or("", |r| r.path.as_str()));
    body.push(if path.is_empty() || wide || v.home.is_some() {
        String::new()
    } else {
        let line = format!("   {}", cockpit::dim(&limit_path(&path, inner.saturating_sub(4), v.style.ascii)));
        mark_matches(&line, &filter_words(v.filter), 3, usize::MAX)
    });
    let action = Some(action_hit(&radio, i32::try_from(1 + body.len()).unwrap_or(0), 0));
    body.push(radio.text);
    let list = Panel {
        title: if list_focused { cockpit::identity(&g) } else { "projects".into() },
        page: if list_focused { title(v, &g) } else { String::new() },
        right: String::new(),
        focus: if list_focused { Focus::Focused } else { Focus::Plain },
        lines: body,
        width: lw,
        height: page_rows,
        painter: &painter,
        selected,
    };
    let mut lines = cockpit::render(&list, v.style);
    let mut side = None;
    if wide {
        let rw = fw - lw;
        let sel = rows.get(index);
        let detail = v.detail.filter(|d| sel.is_some_and(|r| d.key == project_key(&r.path)));
        let facts = selected_lines(sel, detail, v, rw - 2);
        let facts_h = (facts.len() + 2).min(page_rows.saturating_sub(3));
        let title = format!("selected {} {}", g.dot, sel.map_or(String::new(), |r| clean_text(&r.name)));
        let mut right = cockpit::render(&Panel::plain(&title, facts, rw, facts_h), v.style);
        let sessions_h = page_rows - facts_h;
        let listed = session_lines(sel, detail, v, rw - 2, sessions_h.saturating_sub(2), &g);
        right.extend(cockpit::render(&sessions_panel(listed, rw, sessions_h, v.home, &g), v.style));
        lines = cockpit::beside(lines, lw, &right);
        if v.home.is_some() {
            side = session_hits(detail, facts_h, sessions_h).map(|rows| (RowMap::panel_interior(lw, rw), rows));
        }
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
        hit_x: RowMap::panel_interior(0, lw),
        side: side.and_then(|(x, rows)| Some((x?, rows))),
        ..RowMap::default()
    };
    let light = FooterLight {
        hover: frame::footer_hover(&footer, v.hover),
        selected: action_light(v.action),
    };
    let lines = frame::append_footer(lines, &footer, fw, v.style, &light, &mut map);
    Frame { lines, map }
}

/// The selected project's sessions; on the launch page with the sessions focused, the identity
/// and the selected row's fill.
fn sessions_panel<'p>(lines: Vec<String>, width: usize, height: usize, home: Option<Home>, g: &Glyphs) -> Panel<'p> {
    let mut p = Panel::plain("sessions", lines, width, height);
    if let Some(Home {
        focus: HomeFocus::Sessions(i),
        ..
    }) = home
    {
        p.title = cockpit::identity(g);
        "sessions".clone_into(&mut p.page);
        p.focus = Focus::Focused;
        let at = i.min(p.lines.len().saturating_sub(1));
        // The cursor mark as on every list: the fill alone is lost without colour.
        if let Some(line) = p.lines.get_mut(at)
            && let Some(rest) = line.strip_prefix("  ")
        {
            *line = format!(" {}{rest}", g.cursor);
        }
        p.selected = Some(at);
    }
    p
}

/// The label column of the selected panel's facts.
const FACT_LABEL: usize = 10;

fn fact(label: &str, value: &str) -> String {
    format!("  {}{value}", cockpit::dim(&format!("{label:<FACT_LABEL$}")))
}

/// The cursor's project beside the list: its full path, the git branch, its session count and
/// the newest session's title and age.
fn selected_lines(r: Option<&Row>, d: Option<&Detail>, v: &ProjectView<'_>, inner: usize) -> Vec<String> {
    let Some(r) = r else { return Vec::new() };
    let ascii = v.style.ascii;
    if r.kind == RowKind::Path {
        return vec![format!("  {}", cockpit::dim("type a folder path; enter opens it"))];
    }
    let room = inner.saturating_sub(2 + FACT_LABEL + 2);
    let mut out = vec![fact("path", &limit_path(&theme::strip_ctl(&r.path), room, ascii))];
    if r.worktree {
        out.push(fact("worktree", &limit_line(&clean_text(&r.name), room, ascii)));
    }
    let Some(d) = d else {
        out.push(fact(
            "sessions",
            &cockpit::dim(&format!("loading{}", crate::layout::ellipsis(ascii))),
        ));
        return out;
    };
    if let Some(b) = &d.branch {
        out.push(fact("branch", &limit_line(&clean_text(b), room, ascii)));
    }
    out.push(fact("sessions", &d.count.to_string()));
    if let Some(s) = d.sessions.first() {
        // The age leads: right-aligned at the far border it would sit a whole wide column away.
        let age = relative_age(s.modified_ms, v.now_ms);
        let title = limit_line(&clean_text(&s.title), room.saturating_sub(display_width(&age) + 2), ascii);
        out.push(fact("last", &format!("{}  {title}", cockpit::dim(&age))));
    }
    out
}

/// The list panel's width: beside the launch panel the workspace's left column; beside the
/// selected project's facts and sessions the rest of the page, the facts keeping their column (under
/// ~140 columns they give way to `FACTS_MIN`, as on the launch page); alone the whole frame.
fn list_panel_width(v: &ProjectView<'_>, fw: usize, wide: bool) -> usize {
    match v.home {
        Some(h) if wide => h.left_width.min(fw),
        _ if wide => {
            fw - cockpit::FACTS_WIDTH
                .min(fw.saturating_sub(LIST_MIN))
                .max(cockpit::FACTS_MIN)
                .min(fw / 2)
        }
        _ => fw,
    }
}

/// The list's width before the facts beside it give way: a name, a path and an age.
const LIST_MIN: usize = 74;

/// The narrow page's sessions: the selected project's sessions panel alone, `fw` by `page_rows`.
fn sessions_page(v: &ProjectView<'_>, sel: Option<&Row>, (fw, page_rows): (usize, usize), footer: &frame::Footer, g: &Glyphs) -> Frame {
    let detail = v.detail.filter(|d| sel.is_some_and(|r| d.key == project_key(&r.path)));
    let listed = session_lines(sel, detail, v, fw - 2, page_rows.saturating_sub(2), g);
    let lines = cockpit::render(&sessions_panel(listed, fw, page_rows, v.home, g), v.style);
    let mut map = RowMap {
        hit_x: RowMap::panel_interior(0, fw),
        side: session_hits(detail, 0, page_rows).and_then(|rows| Some((RowMap::panel_interior(0, fw)?, rows))),
        ..RowMap::default()
    };
    let light = FooterLight {
        hover: frame::footer_hover(footer, v.hover),
        selected: Vec::new(),
    };
    let lines = frame::append_footer(lines, footer, fw, v.style, &light, &mut map);
    Frame { lines, map }
}

/// The sessions panel's lines in the row map (`SESSION_ROW + i`) under the `facts_h` rows of the
/// selected panel; `None` without a listed session.
fn session_hits(d: Option<&Detail>, facts_h: usize, sessions_h: usize) -> Option<Vec<RowHit>> {
    let n = d?.sessions.len().min(sessions_h.saturating_sub(2));
    (n > 0).then(|| {
        (0..n)
            .map(|i| RowHit {
                index: SESSION_ROW + i,
                name: "session".to_owned(),
                y: i32::try_from(facts_h + 1 + i).unwrap_or(0),
                cells: Vec::new(),
            })
            .collect()
    })
}

/// The selected project's newest sessions, as many as `rows` hold: title, age, a green dot on a
/// running one.
fn session_lines(r: Option<&Row>, d: Option<&Detail>, v: &ProjectView<'_>, inner: usize, rows: usize, g: &Glyphs) -> Vec<String> {
    let ascii = v.style.ascii;
    if r.is_none_or(|r| r.kind == RowKind::Path) {
        return Vec::new();
    }
    let Some(d) = d else {
        return vec![format!("  {}", cockpit::dim(&format!("loading{}", crate::layout::ellipsis(ascii))))];
    };
    if d.sessions.is_empty() {
        return vec![format!("  {}", cockpit::dim("no sessions yet"))];
    }
    let shown = d.sessions.iter().take(rows);
    let ages: Vec<String> = shown.clone().map(|s| relative_age(s.modified_ms, v.now_ms)).collect();
    let age_w = ages.iter().map(|a| display_width(a)).max().unwrap_or(0);
    // Ages first, right-aligned in their own column: the titles take the spare width.
    shown
        .zip(&ages)
        .map(|(s, age)| {
            let dot = if s.live {
                theme::tone(theme::TONE_LIVE, &g.on.to_string())
            } else {
                " ".to_owned()
            };
            let title = limit_line(&clean_text(&s.title), inner.saturating_sub(4 + age_w + 2 + 2), ascii);
            format!("  {dot} {}  {title}", cockpit::dim(&format!("{age:>age_w$}")))
        })
        .collect()
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
