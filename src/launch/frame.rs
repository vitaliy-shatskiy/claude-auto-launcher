//! The launch screen's frame: the cockpit panels around the launch rows. Pure.

use super::{LaunchState, OptionText, Roster, RowName, is_hidden_row_value, visible_row_values};
use crate::cockpit::{self, Focus, Panel};
use crate::frame::{
    self, CellSpan, Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RadioSpec, RowHit, RowMap, Style, measure_cell_spans,
    new_hint_footer, new_radio_row,
};
use crate::layout::{self, display_width};
use crate::projects::view::{Home, HomeFocus};
use crate::theme::{self, Glyphs};
use crate::types::{DefaultLabels, Limits, VersionInfo};

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
    /// The project list and its sessions: panels beside the launch panel on a wide page, pages
    /// after it on a narrow one; `None` while the registry is still being read.
    pub(crate) home: Option<HomePanel<'a>>,
    pub running: &'a [RunningSession],
    /// The last maintenance MCP check (`2 of 2 · 3 h ago`), empty when unknown: never run here.
    pub mcp: &'a str,
}

fn pad_right(s: &str, width: usize) -> String {
    format!("{s:<width$}")
}

/// A running Claude Code session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunningSession {
    pub name: String,
    pub project: String,
    pub age: String,
}

mod panels;
use panels::{claude_code_lines, limit_lines, loading_lines, pin_last, running_lines};

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
    // `only_current`: the row as its selected value alone, for a row whose values do not fit.
    let radio_of = |row: &super::RowDef, prefix: &str, hover: &str, only_current: bool| {
        let texts: Vec<String> = row.values.iter().map(|v| ctx.text(roster, row, v)).collect();
        let mut values = visible_row_values(roster, row, &ctx);
        if only_current {
            values.retain(|v| v == shown_of(row));
        }
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
                // Every row keeps its marks: a row too wide drops to its selected value below.
                max_width: 0,
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
            let radio = radio_of(row, &prefix, if hovered_here { &state.hover_value } else { "" }, false);
            (radio.text, radio.cells)
        };
        // Only a row that does not fit drops to its selected value and the count of the others.
        if display_width(&line) > inner {
            let others = visible_row_values(roster, row, &ctx).len().saturating_sub(1);
            line = format!("{}  +{others}", radio_of(row, &prefix, "", true).text.trim_end());
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
/// selected account's limits beside it, the projects, running sessions and Claude Code's state
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
    if let Some(f) = narrow_page(inp) {
        return f;
    }
    let g = theme::glyphs(inp.style.ascii);
    let fw = frame::frame_width(inp.width);
    let wide = inp.width >= cockpit::TWO_COLUMNS;
    let rw = if wide { facts_width(roster, inp, &restored, fw, &g) } else { 0 };
    let lw = fw - rw;
    let rows = option_rows(roster, inp, &restored, lw.saturating_sub(3), &g);
    let selected = rows.iter().position(|r| r.0 == state.row);
    let mut map = RowMap {
        rows: launch_hits(&rows),
        ..RowMap::default()
    };
    let home = inp.home;
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
            home: home.is_some(),
        },
    );
    let painter = |l: &str, enabled: bool| add_launch_color(l, enabled, &g, roster);
    let account = layout::limit_line(&state.account, 30, inp.style.ascii);
    let lim_inner = if wide { rw.saturating_sub(2) } else { fw.saturating_sub(2) };
    let (lims, age) = limit_lines(inp.limits.get(&state.account), lim_inner, !wide);
    let lim_need = lims.len() + usize::from(!age.is_empty()) + 2;
    let launch_need = launch_lines.len() + 2;
    let top_h = if wide { launch_need.max(lim_need) } else { launch_need };
    let right = versions(inp.version, &g, wide);
    // The identity goes with the focus: off the launch rows, the panel is plain `launch`.
    let launch_focused = home.is_none_or(|h| h.focus == HomeFocus::Launch);
    let launch = Panel {
        title: if launch_focused { cockpit::identity(&g) } else { "launch".into() },
        page: String::new(),
        right,
        focus: if launch_focused { Focus::Focused } else { Focus::Plain },
        lines: launch_lines,
        width: lw,
        height: top_h,
        painter: &painter,
        selected,
    };
    let page_rows = inp.height.saturating_sub(1 + footer.lines.len());
    // Stacked, the limits run to the footer (AC27).
    let lim_h = if wide {
        top_h
    } else {
        page_rows.saturating_sub(launch_need).max(lim_need)
    };
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
    let mut lines = if wide {
        cockpit::beside(cockpit::render(&launch, inp.style), lw, &cockpit::render(&limits, inp.style))
    } else {
        let mut l = cockpit::render(&launch, inp.style);
        l.extend(cockpit::render(&limits, inp.style));
        l
    };
    if let Some(h) = home.filter(|_| wide) {
        return home::home_frame(inp, h, lines, map, (lw, rw), &footer);
    }
    let left_over = page_rows.saturating_sub(lines.len());
    if wide {
        lines.extend(bottom_panels(inp, (lw, rw), left_over, &g));
    }
    map.hit_x = RowMap::panel_interior(0, lw);
    let light = FooterLight {
        hover: frame::footer_hover(&footer, state.hover),
        selected: Vec::new(),
    };
    let lines = frame::append_footer(lines, &footer, fw, inp.style, &light, &mut map);
    Frame { lines, map }
}

/// Narrow, the list and the sessions are pages after the launch page, in the same frame: the
/// focused one drawn whole; `None` on the launch page or a wide terminal.
fn narrow_page(inp: &LaunchFrameInput<'_>) -> Option<Frame> {
    let h = inp
        .home
        .filter(|h| inp.width < cockpit::TWO_COLUMNS && h.focus != HomeFocus::Launch)?;
    let page = Home {
        focus: h.focus,
        left_width: frame::frame_width(inp.width),
    };
    Some((h.draw)(page, inp.width, inp.height))
}

/// The facts column beside the launch panel: `FACTS_WIDTH` cells, the left column the rest; on a
/// page too narrow for both the facts give way, down to `FACTS_MIN`, so every launch row keeps its
/// values.
fn facts_width(roster: &Roster, inp: &LaunchFrameInput<'_>, restored: &[String], fw: usize, g: &Glyphs) -> usize {
    let need = option_rows(roster, inp, restored, fw, g)
        .iter()
        .map(|r| display_width(&theme::strip_ctl(&r.2)) + 4)
        .max()
        .unwrap_or(0);
    cockpit::FACTS_WIDTH
        .min(fw.saturating_sub(need))
        .max(cockpit::FACTS_MIN)
        .min(fw / 2)
}

/// The launch rows' hits, their cells measured from the panel's first interior cell.
fn launch_hits(rows: &[(usize, &'static str, String, Vec<CellSpan>)]) -> Vec<RowHit> {
    rows.iter()
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
        .collect()
}

/// The bottom row's height beside: to the footer (AC27).
fn bottom_height(left_over: usize) -> usize {
    left_over.max(3 + STATUS_HEIGHT)
}

/// The Claude Code panel: its two lines and the borders.
const STATUS_HEIGHT: usize = 4;

/// While the registry is read: the projects panel saying so, running sessions and Claude Code's
/// state beside it, to the footer.
fn bottom_panels(inp: &LaunchFrameInput<'_>, (lw, rw): (usize, usize), left_over: usize, g: &Glyphs) -> Vec<String> {
    let style = inp.style;
    let status_h = STATUS_HEIGHT;
    let bh = bottom_height(left_over);
    let projects = Panel::plain("projects", loading_lines(style.ascii), lw, bh);
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
    cockpit::beside(cockpit::render(&projects, style), lw, &right)
}

/// `0.9.0`, the launcher; stacked, where no Claude Code panel shows, `0.9.0 · Claude Code 2.1.330
/// ↑ 2.1.331` (the installed Claude Code and a newer one). Beside, its panel holds them.
fn versions(v: &VersionInfo, g: &Glyphs, panel: bool) -> String {
    let mut s = env!("CARGO_PKG_VERSION").to_owned();
    if let Some(i) = v.installed.as_deref().filter(|s| !s.is_empty() && !panel) {
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
    if extra.home {
        hints.push(Hint::key("tab", "projects", "Tab"));
    }
    hints.push(Hint::ch("u", if short { "maint" } else { "maintenance" }, 'u').badged(badge));
    if extra.restored {
        hints.push(Hint::plain(if short { "^r" } else { "ctrl+r" }, "reset").kept());
    }
    hints.push(Hint::key("esc", "quit", "Escape"));
    // One line: what does not fit goes, ctrl+r first, then w/s (the arrows need no hint).
    for drop in ["^r", "w/s"] {
        let footer = new_hint_footer(&hints, g, width, !color);
        if footer.lines.iter().map(|l| l.spans.len()).sum::<usize>() == hints.len() {
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
    /// The projects panel is on the page: `tab projects` is offered.
    pub home: bool,
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
            // The selected account keeps its tint under the pointer: a click draws what a key draws.
            if hovered_here && v.eq_ignore_ascii_case(&state.hover_value) && !v.eq_ignore_ascii_case(&state.account) {
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

mod color;
use color::add_launch_color;

mod home;
pub(crate) use home::HomePanel;

#[cfg(test)]
mod tests;
