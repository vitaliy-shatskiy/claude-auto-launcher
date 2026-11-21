//! The launch screen's frame: the cockpit panels around the launch rows. Pure.

use std::sync::LazyLock;

use regex::Regex;

use super::{LaunchState, OptionText, Roster, RowName, is_hidden_row_value, visible_row_values};
use crate::cockpit::{self, Focus, Panel};
use crate::frame::{
    self, CellSpan, Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RadioSpec, RowHit, RowMap, Style, measure_cell_spans,
    new_hint_footer, new_radio_row,
};
use crate::layout::{self, display_width};
use crate::theme::{self, Glyphs};
use crate::types::{DefaultLabels, LimitRecord, Limits, VersionInfo};

/// Everything the launch frame draws from.
#[derive(Debug, Clone, Copy)]
pub struct LaunchFrameInput<'a> {
    pub state: &'a LaunchState,
    pub width: usize,
    pub height: usize,
    pub limits: &'a Limits,
    pub version: &'a VersionInfo,
    /// Explicit restore marks and age; `None` reads them off the state (per tab).
    pub restored: Option<(&'a [String], &'a str)>,
    pub default_model_label: &'a str,
    pub default_advisor_label: &'a str,
    pub default_labels: &'a DefaultLabels,
    pub style: Style,
    /// The self-update line, drawn dim on the page's last row.
    pub notice: Option<&'a str>,
    /// How many warnings maintenance holds: the footer's badge.
    pub warnings: usize,
    /// Newest first; keys 1-5 continue the first five.
    pub recent: &'a [RecentProject],
    /// The registry is still being read: the empty recent panel says so instead of "none yet".
    pub recent_loading: bool,
    pub running: &'a [RunningSession],
    /// The last maintenance MCP check (`2 of 2 · 3 h ago`), empty when unknown: never run here.
    pub mcp: &'a str,
}

fn pad_right(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

/// A recent project on the launch page: keys 1-5 continue its newest session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentProject {
    pub name: String,
    pub path: String,
    /// The slug of its newest activity and every slug of the directory, as the project screen
    /// hands them to the launch.
    pub slug: String,
    pub slugs: Vec<String>,
    /// `3 min`, `2 d`: how long ago its newest session ran.
    pub age: String,
    /// Its newest session's title from the summary cache; empty when none is cached.
    pub last: String,
}

/// A running Claude Code session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunningSession {
    pub name: String,
    pub project: String,
    pub age: String,
}

/// The limit lines of the selected account: one per limit, label dim, percentage bold, coloured
/// and right-aligned 2 cells from the border; `None` without a record.
fn limit_lines(l: Option<&LimitRecord>, inner: usize, one_line: bool) -> (Vec<String>, String) {
    let Some(l) = l.filter(|l| l.five_hour.is_some() || l.seven_day.is_some() || l.model.is_some()) else {
        return (
            vec![
                format!("  {}", cockpit::dim("no data yet")),
                format!("  {}", theme::tone(theme::TONE_FAINT, "fills in after the first session")),
            ],
            String::new(),
        );
    };
    let label = l
        .model_label
        .as_deref()
        .filter(|s| !s.is_empty())
        .map_or_else(|| "model".to_owned(), str::to_lowercase);
    let label = l.model_age.as_deref().map_or(label.clone(), |a| format!("{label} ({a})"));
    let items = [("5h".to_owned(), l.five_hour), ("7d".to_owned(), l.seven_day), (label, l.model)];
    let tone_of = |p: i64| {
        if p >= 90 {
            theme::TONE_ERROR
        } else if p >= 70 {
            theme::TONE_WARN
        } else {
            theme::TONE_STRONG
        }
    };
    let shown = items.iter().filter_map(|(k, p)| p.map(|p| (k, p)));
    if one_line {
        let text: Vec<String> = shown
            .map(|(k, p)| format!("{} {}", cockpit::dim(k), theme::tone(tone_of(p), &format!("{p}%"))))
            .collect();
        let age = cockpit::dim(&format!("data {}", l.age_text));
        return (
            vec![cockpit::spread(&format!("  {}", text.join("  ")), &age, inner.saturating_sub(2))],
            String::new(),
        );
    }
    let lines = shown
        .map(|(k, p)| {
            let tone = tone_of(p);
            let left = format!("  {}", cockpit::dim(&layout::limit_line(k, inner.saturating_sub(10), false)));
            cockpit::spread(&left, &theme::tone(tone, &format!("{p}%")), inner.saturating_sub(2))
        })
        .collect();
    let age = if l.age_text.is_empty() {
        String::new()
    } else {
        format!("  {}", cockpit::dim(&format!("data {}", l.age_text)))
    };
    (lines, age)
}

/// The panel's lines with `last` pinned to its last interior row.
fn pin_last(mut lines: Vec<String>, last: String, height: usize) -> Vec<String> {
    if last.is_empty() {
        return lines;
    }
    lines.resize(height.saturating_sub(3).max(lines.len()), String::new());
    lines.push(last);
    lines
}

/// The widest a recent row is laid out (the chat's 110-cell text and its insets).
const RECENT_TEXT: usize = 114;

/// Recent projects: key, name and age, the last session's title dim under it; one line each when
/// `two_line` is off.
fn recent_lines(inp: &LaunchFrameInput<'_>, inner: usize, rows: usize, two_line: bool, hover: Option<usize>) -> Vec<String> {
    let (recent, ascii) = (inp.recent, inp.style.ascii);
    if recent.is_empty() && inp.recent_loading {
        return vec![format!(
            "  {}",
            cockpit::dim(&format!("loading recent projects{}", layout::ellipsis(ascii)))
        )];
    }
    if recent.is_empty() {
        return vec![
            format!("  {}", cockpit::dim("no recent projects yet")),
            format!("  {}", theme::tone(theme::TONE_FAINT, "enter opens the project list")),
        ];
    }
    let per = if two_line { 2 } else { 1 };
    // On a wide page the age stays near its name: rows are laid out in at most RECENT_TEXT cells.
    let inner = inner.min(RECENT_TEXT);
    let mut out = Vec::new();
    for (i, p) in recent.iter().take((rows / per).min(5)).enumerate() {
        let age = cockpit::dim(&p.age);
        let room = inner.saturating_sub(5 + display_width(&p.age) + 4);
        let name = layout::limit_line(&p.name, room, ascii);
        let name = if hover == Some(i) {
            theme::add_hover_span(&name)
        } else if i == 0 {
            theme::tone(theme::TONE_STRONG, &name)
        } else {
            name
        };
        let left = format!("  {}  {name}", theme::tone(theme::TONE_ACCENT, &(i + 1).to_string()));
        out.push(cockpit::spread(&left, &age, inner.saturating_sub(2)));
        // The newest session's title; none cached yet leaves the line blank (the row keeps its rhythm).
        if two_line {
            out.push(format!(
                "     {}",
                cockpit::dim(&layout::limit_line(&p.last, inner.saturating_sub(7), ascii))
            ));
        }
    }
    out
}

fn running_lines(running: &[RunningSession], inner: usize, rows: usize, g: &Glyphs) -> Vec<String> {
    if running.is_empty() {
        return vec![format!("  {}", cockpit::dim("nothing running"))];
    }
    let mut out = Vec::new();
    for r in running.iter().take(rows / 2) {
        let name = layout::limit_line(&r.name, inner.saturating_sub(8 + display_width(&r.age)), g.on == '*');
        let left = format!("  {} {name}", theme::tone(theme::TONE_LIVE, &g.on.to_string()));
        out.push(cockpit::spread(&left, &cockpit::dim(&r.age), inner.saturating_sub(2)));
        out.push(format!(
            "    {}",
            theme::tone(
                theme::TONE_FAINT,
                &layout::limit_line(&r.project, inner.saturating_sub(6), g.on == '*')
            )
        ));
    }
    out
}

/// Claude Code's state: the version (and the newest when it differs), the warnings count.
fn claude_code_lines(version: &VersionInfo, warnings: usize, mcp: &str, g: &Glyphs) -> Vec<String> {
    let installed = version.installed.as_deref().filter(|s| !s.is_empty());
    let newest = version.newest.as_deref().filter(|s| !s.is_empty());
    let first = match (installed, newest) {
        (Some(i), Some(n)) if n != i => format!(
            "  {}  {}",
            theme::tone(theme::TONE_STRONG, i),
            cockpit::dim(&format!("{} {n}", g.up))
        ),
        (Some(i), Some(_)) => format!("  {}  {}", theme::tone(theme::TONE_STRONG, i), cockpit::dim("newest")),
        (Some(i), None) => format!("  {}", theme::tone(theme::TONE_STRONG, i)),
        (None, _) => format!("  {}", cockpit::dim("not found")),
    };
    let mut out = vec![first];
    let mcp = (!mcp.is_empty()).then(|| format!("{} {}", cockpit::dim("MCP"), layout::limit_line(mcp, 30, g.on == '*')));
    if warnings > 0 {
        let s = if warnings == 1 { "" } else { "s" };
        let warn = format!(
            "{} {warnings} warning{s} {} u",
            theme::tone(theme::TONE_WARN, &g.warn.to_string()),
            g.dot
        );
        out.push(match mcp {
            Some(m) => format!("  {m}   {warn}"),
            None => format!("  {warn}"),
        });
    } else if let Some(m) = mcp {
        out.push(format!("  {m}"));
    }
    out
}

/// The launch rows: one line each, their click cells measured from the line's first cell.
fn option_rows(
    roster: &Roster,
    inp: &LaunchFrameInput<'_>,
    restored: &[String],
    inner: usize,
    g: &Glyphs,
) -> Vec<(usize, &'static str, String, Vec<CellSpan>)> {
    let state = inp.state;
    let label_width = roster.rows.iter().map(|r| r.label.len()).max().unwrap_or(0) + 3;
    let limit = inp.limits.get(&state.account);
    let available = limit.and_then(|l| l.available_models.as_deref());
    let ctx = OptionText {
        default_model_label: inp.default_model_label,
        default_advisor_label: inp.default_advisor_label,
        available,
        default_labels: inp.default_labels,
    };
    let hover_row = state.hover_row;
    let mark_of = |row: &super::RowDef| {
        if restored.iter().any(|r| r.eq_ignore_ascii_case(row.name.as_str())) {
            "*"
        } else {
            " "
        }
    };
    let shown_of = |row: &super::RowDef| {
        let current = state.get(row.name);
        if is_hidden_row_value(roster, row, current, &ctx) {
            "default"
        } else {
            current
        }
    };
    let radio_of = |row: &super::RowDef, prefix: &str, hover: &str| {
        let texts: Vec<String> = row.values.iter().map(|v| ctx.text(roster, row, v)).collect();
        let values = visible_row_values(roster, row, &ctx);
        let vtexts: Vec<String> = values
            .iter()
            .map(|v| {
                row.values
                    .iter()
                    .position(|x| x == v)
                    .and_then(|p| texts.get(p).cloned())
                    .unwrap_or_else(|| ctx.text(roster, row, v))
            })
            .collect();
        new_radio_row(
            &RadioSpec {
                prefix,
                label: &format!("{}{}", row.label, mark_of(row)),
                values: &values,
                current: shown_of(row),
                texts: &vtexts,
                label_width,
                max_width: if inp.width >= frame::TWO_PANE_WIDTH { inner } else { 0 },
                hover,
                compact: false,
            },
            g,
        )
    };
    let mut out = Vec::new();
    for (i, row) in roster.rows.iter().enumerate() {
        let prefix = if state.row == i {
            format!(" {} ", g.cursor)
        } else {
            "   ".to_owned()
        };
        let label_part = pad_right(&format!("{}{}", row.label, mark_of(row)), label_width);
        let hovered_here = i32::try_from(i).is_ok_and(|ii| ii == hover_row);
        let (mut line, mut cells) = if row.name == RowName::Account {
            account_strip(row, state, inp.limits, &prefix, &label_part, inner, g, hovered_here)
        } else {
            let radio = radio_of(row, &prefix, if hovered_here { &state.hover_value } else { "" });
            (radio.text, radio.cells)
        };
        // Only a row that does not fit collapses to `< value >`: the others stay readable.
        if display_width(&line) > inner {
            line = format!(
                "{prefix}{label_part}{} {} {}",
                g.l_angle,
                ctx.text(roster, row, shown_of(row)),
                g.r_angle
            );
            cells = Vec::new();
        }
        // The pointer on the row outside its values paints the label: hover never selects.
        if hovered_here
            && state.hover_value.is_empty()
            && let Some(rest) = line.strip_prefix(&format!("{prefix}{label_part}"))
        {
            let label = format!("{}{}", row.label, mark_of(row));
            let pad = " ".repeat(label_width.saturating_sub(label.len()));
            line = format!("{prefix}{}{pad}{rest}", theme::add_hover_span(&label));
        }
        out.push((i, row.name.as_str(), line, cells));
    }
    out
}

/// The launch page: the launch panel (focused, carrying the identity and the versions), the
/// selected account's limits beside it, recent projects, running sessions and Claude Code's state
/// under them; under `cockpit::TWO_COLUMNS` columns the panels stack.
pub fn launch_frame(roster: &Roster, inp: &LaunchFrameInput<'_>) -> Frame {
    let state = inp.state;
    let restored: Vec<String> = inp.restored.map_or_else(|| state.restored.clone(), |(r, _)| r.to_vec());
    if inp.width < MIN_WIDTH || inp.height < MIN_HEIGHT {
        return Frame {
            lines: frame::too_small_frame(inp.width, inp.height, inp.style.ascii),
            map: RowMap::default(),
        };
    }
    let g = theme::glyphs(inp.style.ascii);
    let fw = frame::frame_width(inp.width);
    let wide = inp.width >= cockpit::TWO_COLUMNS;
    // The facts column keeps its width; the launch and recent panels take the spare cells.
    let rw = if wide { cockpit::FACTS_WIDTH.min(fw / 2) } else { 0 };
    let lw = fw - rw;
    let rows = option_rows(roster, inp, &restored, lw.saturating_sub(3), &g);
    let selected = rows.iter().position(|r| r.0 == state.row);
    let mut map = RowMap {
        rows: rows
            .iter()
            .enumerate()
            .map(|(y, (index, name, _, cells))| RowHit {
                index: *index,
                name: (*name).to_owned(),
                y: i32::try_from(1 + y).unwrap_or(0),
                cells: cells
                    .iter()
                    .map(|c| CellSpan {
                        start: c.start + 1,
                        end: c.end + 1,
                        value: c.value.clone(),
                    })
                    .collect(),
            })
            .collect(),
        ..RowMap::default()
    };
    let mut launch_lines: Vec<String> = rows.into_iter().map(|r| r.2).collect();
    // The self-update notice closes the launch panel, dim, a blank row under the rows.
    if let Some(n) = inp.notice {
        launch_lines.push(String::new());
        launch_lines.push(format!("  {}", cockpit::dim(n)));
    }
    let footer = launch_footer(
        &g,
        fw,
        inp.style.color,
        &LaunchHints {
            warnings: inp.warnings,
            restored: !restored.is_empty(),
            recent: !inp.recent.is_empty(),
        },
    );
    let painter = |l: &str, enabled: bool| add_launch_color(l, enabled, &g, roster);
    let account = layout::limit_line(&state.account, 30, inp.style.ascii);
    let lim_inner = if wide { rw.saturating_sub(2) } else { fw.saturating_sub(2) };
    let (lims, age) = limit_lines(inp.limits.get(&state.account), lim_inner, !wide);
    let lim_need = lims.len() + usize::from(!age.is_empty()) + 2;
    let launch_need = launch_lines.len() + 2;
    let top_h = if wide { launch_need.max(lim_need) } else { launch_need };
    let right = versions(inp.version, &g);
    let top_lw = if wide { launch_width(&launch_lines, &right, lw, &g) } else { lw };
    let launch = Panel {
        title: cockpit::identity(&g),
        page: String::new(),
        right,
        focus: Focus::Focused,
        lines: launch_lines,
        width: top_lw,
        height: top_h,
        painter: &painter,
        selected,
    };
    let lim_h = if wide { top_h } else { lim_need };
    let limits = Panel {
        title: format!("limits {} {account}", g.dot),
        page: String::new(),
        right: String::new(),
        focus: Focus::Plain,
        lines: pin_last(lims, age, lim_h),
        width: if wide { rw } else { fw },
        height: lim_h,
        painter: &cockpit::no_paint,
        selected: None,
    };
    let page_rows = inp.height.saturating_sub(1 + footer.lines.len());
    let mut lines = if wide {
        cockpit::beside(cockpit::render(&launch, inp.style), top_lw, &cockpit::render(&limits, inp.style))
    } else {
        let mut l = cockpit::render(&launch, inp.style);
        l.extend(cockpit::render(&limits, inp.style));
        l
    };
    let left_over = page_rows.saturating_sub(lines.len());
    let top = lines.len();
    lines.extend(bottom_panels(inp, wide, (lw, rw), left_over, &g));
    map.rows.extend(recent_hits(inp, wide, left_over, top));
    let light = FooterLight {
        hover: frame::footer_hover(&footer, state.hover),
        selected: Vec::new(),
    };
    let lines = frame::append_footer(lines, &footer, fw, inp.style, &light, &mut map);
    Frame { lines, map }
}

/// Beside the limits the launch panel is as wide as its rows and its title need, at most `most`:
/// the recent list under it takes the spare width.
fn launch_width(lines: &[String], right: &str, most: usize, g: &Glyphs) -> usize {
    let rows_w = lines.iter().map(|l| display_width(&theme::strip_ctl(l))).max().unwrap_or(0) + 4;
    let title_w = display_width(&cockpit::identity(g)) + display_width(right) + 10;
    rows_w.max(title_w).min(most)
}

/// The row index a recent project's line has in the row map: `RECENT_ROW + i`, past every launch row.
pub const RECENT_ROW: usize = 1000;

/// The recent panel's lines in the row map, under the top panels' `top` rows.
fn recent_hits(inp: &LaunchFrameInput<'_>, wide: bool, left_over: usize, top: usize) -> Vec<RowHit> {
    let per = if wide { 2 } else { 1 };
    (0..recent_shown(inp, wide, left_over))
        .map(|i| RowHit {
            index: RECENT_ROW + i,
            name: "recent".to_owned(),
            y: i32::try_from(top + 1 + i * per).unwrap_or(0),
            cells: Vec::new(),
        })
        .collect()
}

/// How many recent projects the recent panel lists: one per row stacked, one per two rows beside.
fn recent_shown(inp: &LaunchFrameInput<'_>, wide: bool, left_over: usize) -> usize {
    let rows = if wide {
        bottom_height(inp, left_over).saturating_sub(2) / 2
    } else if left_over >= 3 {
        left_over - 2
    } else {
        0
    };
    inp.recent.len().min(rows).min(5)
}

/// The bottom row's height beside: to the footer with items, the empty-state lines without.
fn bottom_height(inp: &LaunchFrameInput<'_>, left_over: usize) -> usize {
    if inp.recent.is_empty() && inp.running.is_empty() {
        3 + STATUS_HEIGHT
    } else {
        left_over.max(3 + STATUS_HEIGHT)
    }
}

/// The Claude Code panel: its two lines and the borders.
const STATUS_HEIGHT: usize = 4;

/// Recent projects, running sessions and Claude Code's state under the top panels: they stretch to
/// the footer when a list has items and shrink to their empty-state lines when both are empty.
/// Stacked, only the recent panel shows, and only when `left_over` rows hold it.
fn bottom_panels(inp: &LaunchFrameInput<'_>, wide: bool, (lw, rw): (usize, usize), left_over: usize, g: &Glyphs) -> Vec<String> {
    let style = inp.style;
    let hover = usize::try_from(inp.state.hover_row).ok().and_then(|r| r.checked_sub(RECENT_ROW));
    if !wide {
        if left_over < 3 {
            return Vec::new();
        }
        let rh = if inp.recent.is_empty() { 4.min(left_over) } else { left_over };
        let lines = recent_lines(inp, lw - 2, rh - 2, false, hover);
        return cockpit::render(&Panel::plain("recent", lines, lw, rh), style);
    }
    let status_h = STATUS_HEIGHT;
    let bh = bottom_height(inp, left_over);
    let recent = Panel::plain("recent", recent_lines(inp, lw - 2, bh - 2, true, hover), lw, bh);
    let running = Panel::plain(
        "running",
        running_lines(inp.running, rw - 2, bh - status_h - 2, g),
        rw,
        bh - status_h,
    );
    let status = Panel::plain(
        "claude code",
        claude_code_lines(inp.version, inp.warnings, inp.mcp, g),
        rw,
        status_h,
    );
    let mut right = cockpit::render(&running, style);
    right.extend(cockpit::render(&status, style));
    cockpit::beside(cockpit::render(&recent, style), lw, &right)
}

/// `0.9.0 · Claude Code 2.1.330 ↑ 2.1.331`: the launcher, the installed Claude Code and a newer one.
fn versions(v: &VersionInfo, g: &Glyphs) -> String {
    let mut s = env!("CARGO_PKG_VERSION").to_owned();
    if let Some(i) = v.installed.as_deref().filter(|s| !s.is_empty()) {
        s = format!("{s} {} Claude Code {i}", g.dot);
        if let Some(n) = v.newest.as_deref().filter(|n| !n.is_empty() && *n != i) {
            s = format!("{s} {} {n}", g.up);
        }
    }
    s
}

/// The footer: arrows are not clickable ("w/s" names two directions), every single action is.
/// Under 110 columns the long words shorten and `ctrl+r` goes first.
pub fn launch_footer(g: &Glyphs, width: usize, color: bool, extra: &LaunchHints) -> Footer {
    let short = width < 110;
    let badge = if extra.warnings > 0 {
        format!("{} {}", g.warn, extra.warnings)
    } else {
        String::new()
    };
    let mut hints = vec![
        Hint::plain("w/s", "row").kept(),
        Hint::plain(if short { "a/d" } else { frame::switch_keys(g) }, "switch").kept(),
        Hint::key("enter", "next", "Enter"),
    ];
    if extra.recent {
        hints.push(Hint::plain("1-5", if short { "go" } else { "resume" }).kept());
    }
    hints.push(Hint::ch("u", if short { "maint" } else { "maintenance" }, 'u').badged(badge));
    if extra.restored {
        hints.push(Hint::plain(if short { "^r" } else { "ctrl+r" }, "reset").kept());
    }
    hints.push(Hint::key("esc", "quit", "Escape"));
    // One line: what does not fit goes, ctrl+r first, then w/s (the arrows need no hint).
    for drop in ["^r", "w/s"] {
        let footer = new_hint_footer(&hints, g, width, !color);
        if footer.lines.len() <= 1 {
            return footer;
        }
        hints.retain(|h| h.token != drop);
    }
    new_hint_footer(&hints, g, width, !color)
}

/// What the launch footer shows besides its fixed keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchHints {
    /// Collected warnings: a yellow `⚠ N` badge on `u maintenance`.
    pub warnings: usize,
    /// A row was restored: `ctrl+r reset` is offered.
    pub restored: bool,
    /// Recent projects are listed: `1-5 resume` is offered.
    pub recent: bool,
}

#[expect(
    clippy::too_many_arguments,
    reason = "the strip reads the row, the state and the geometry together"
)]
fn account_strip(
    row: &super::RowDef,
    state: &LaunchState,
    limits: &Limits,
    prefix: &str,
    label_part: &str,
    inner: usize,
    g: &Glyphs,
    hovered_here: bool,
) -> (String, Vec<CellSpan>) {
    let joiner = format!(" {} ", g.hint_sep);
    let mut cells: Vec<String> = Vec::new();
    for with_percent in [true, false] {
        cells = row
            .values
            .iter()
            .map(|v| {
                let text = match limits.get(v).and_then(|l| l.five_hour) {
                    Some(p) if with_percent => format!("{v} {p}%"),
                    _ => v.clone(),
                };
                if v.eq_ignore_ascii_case(&state.account) {
                    format!("[{text}]")
                } else {
                    text
                }
            })
            .collect();
        if display_width(&format!("{prefix}{label_part}{}", cells.join(&joiner))) <= inner {
            break;
        }
    }
    let draw: Vec<String> = cells
        .iter()
        .zip(&row.values)
        .map(|(c, v)| {
            if hovered_here && v.eq_ignore_ascii_case(&state.hover_value) {
                theme::add_hover_span(c)
            } else {
                c.clone()
            }
        })
        .collect();
    let line = format!("{prefix}{label_part}{}", draw.join(&joiner));
    let spans = measure_cell_spans(&cells, &joiner, display_width(&format!("{prefix}{label_part}")), &row.values);
    (line, spans)
}

static SGR_ROW_LABEL: LazyLock<Regex> = LazyLock::new(|| theme::re(r"^.{3}account[\*\s]"));
static COMPACT_PREFIX: LazyLock<Regex> = LazyLock::new(|| theme::re("^.{3}[a-z]+\\*? +(?:[^\\s\\[\\]]+ )*\u{4}?$"));
static BRACKET: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\[([^\[\]]+)\]"));
static PERCENT: LazyLock<Regex> = LazyLock::new(|| theme::re(r"(\d+)%"));
static ROW_WORDS: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\b(account|action|model|effort|advisor|permission|remote|mode)\b"));
static AGE: LazyLock<Regex> = LazyLock::new(|| theme::re(r"(\d+ (?:min|h|d) ago|just now)"));

/// The selected cell's bracket `[...]` with at most one nested pair, starting at `open`
/// (a char index of `[`); returns the exclusive end char index.
fn selected_bracket_end(chars: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    loop {
        match chars.get(i)? {
            ']' => return Some(i + 1),
            '[' => {
                let mut j = i + 1;
                loop {
                    match chars.get(j)? {
                        ']' => break,
                        '[' => return None,
                        _ => j += 1,
                    }
                }
                i = j + 1;
            }
            _ => i += 1,
        }
    }
}

fn is_selected_anchor(chars: &[char], open: usize, on: char) -> bool {
    if open >= 2 && chars.get(open - 2) == Some(&on) && chars.get(open - 1) == Some(&' ') {
        return true;
    }
    let prefix: String = chars.get(..open).map_or_else(String::new, |c| c.iter().collect());
    let not_account = prefix.chars().skip(3).collect::<String>();
    COMPACT_PREFIX.is_match(&prefix) && !not_account.starts_with("account")
}

/// Painted AFTER layout; the selected cell first (anchored on the On glyph or
/// the compact row's label), then the account tab, percentages, row words, cursor, sparkle, ages.
pub fn add_launch_color(line: &str, enabled: bool, g: &Glyphs, roster: &Roster) -> String {
    if !enabled || line.is_empty() {
        return line.to_owned();
    }
    let is_account_row = SGR_ROW_LABEL.is_match(line);
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars.get(i) == Some(&'[')
            && is_selected_anchor(&chars, i, g.on)
            && let Some(end) = selected_bracket_end(&chars, i)
        {
            let whole: String = chars.get(i..end).map_or_else(String::new, |c| c.iter().collect());
            let value: String = chars.get(i + 1..end - 1).map_or_else(String::new, |c| c.iter().collect());
            let tint = if ["off", "stop server", "safe"].iter().any(|v| v.eq_ignore_ascii_case(&value)) {
                theme::YELLOW
            } else {
                roster.tint(&value)
            };
            out.push_str(theme::BOLD);
            out.push_str(tint);
            out.push_str(&whole);
            out.push_str(theme::RESET);
            i = end;
            continue;
        }
        if let Some(c) = chars.get(i) {
            out.push(*c);
        }
        i += 1;
    }
    if is_account_row {
        out = BRACKET
            .replace_all(&out, |c: &regex::Captures<'_>| {
                let whole = c.get(0).map_or("", |m| m.as_str());
                let value = c.get(1).map_or("", |m| m.as_str()).split(' ').next().unwrap_or_default();
                format!("{}{}{whole}{}", theme::BOLD, roster.tint(value), theme::RESET)
            })
            .into_owned();
    }
    out = PERCENT
        .replace_all(&out, |c: &regex::Captures<'_>| {
            let p = c.get(1).map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(i64::MAX));
            format!("{}{}{}", theme::percent_color(p), c.get(0).map_or("", |m| m.as_str()), theme::RESET)
        })
        .into_owned();
    out = ROW_WORDS
        .replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned();
    out = out.replace(g.cursor, &format!("{}{}{}{}", theme::BOLD, cockpit::ACCENT, g.cursor, theme::RESET));
    out = out.replace(g.sparkle, &format!("{}{}{}", theme::ACCENT, g.sparkle, theme::RESET));
    AGE.replace_all(&out, format!("{}${{1}}{}", theme::DIM, theme::RESET).as_str())
        .into_owned()
}

#[cfg(test)]
mod tests;
