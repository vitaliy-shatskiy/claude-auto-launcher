//! The maintenance page, pure, in the cockpit frame: the actions in groups (title in the accent,
//! chip and name on the left, the hint or the live state right-aligned) in the focused panel; the
//! last run, the versions and the warnings in their own panels.

use crate::cockpit::{self, Focus, Panel};
use crate::frame::{
    Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RowHit, RowMap, Style, append_footer, footer_hover, frame_width,
    new_hint_footer, too_small_frame,
};
use crate::layout::{display_width, limit_line};
use crate::theme::{self, DIM_CLOSE, DIM_OPEN, Glyphs, add_hover_span, glyphs, strip_ctl};

use super::super::install::InstallInfo;
use super::gib;

/// The group an action sits under, in drawing order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Group {
    #[default]
    ClaudeCode,
    Checks,
    ClaudeAuto,
    Launcher,
    Custom,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Checks => "Checks",
            Self::ClaudeAuto => "claude-auto",
            Self::Launcher => "Launcher",
            Self::Custom => "Custom",
        }
    }
}

/// One action row: its key, its name, the hint the selected row shows ('' = none), its group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) key: char,
    pub(crate) name: String,
    pub(crate) about: String,
    pub(crate) group: Group,
}

/// Where the last action stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Idle,
    /// A confirm-twice key waits for its second press.
    Armed,
    /// The live step line (`updating · 3 s · downloading 45 %`).
    Running(String),
    Done,
    Failed(String),
}

/// Everything the page is drawn from.
#[derive(Debug, Clone, Default)]
pub(crate) struct Page {
    pub(crate) info: InstallInfo,
    /// `claude --version` as read at start; `None` when it could not run (preview).
    pub(crate) installed: Option<String>,
    /// The release channel's newest version, when known.
    pub(crate) newest: Option<String>,
    /// claude-auto's self-update mode and state, in words (`self-update notify, up to date`).
    pub(crate) launcher: String,
    /// In drawing order: grouped.
    pub(crate) entries: Vec<Entry>,
    pub(crate) selected: usize,
    pub(crate) warnings: Vec<String>,
    pub(crate) config_path: String,
    /// The name of the action the phase belongs to.
    pub(crate) last: String,
    pub(crate) phase: Phase,
    /// The last action's report.
    pub(crate) status: String,
    /// The hovered footer button (-1 none).
    pub(crate) hover: i32,
    /// The action row under the pointer (-1 none): painted, never selected.
    pub(crate) hover_row: i32,
}

// Colour markers, resolved by the page's painter after layout (zero cells wide, stripped plain).
const GREEN_OPEN: char = '\u{6}';
const RED_OPEN: char = '\u{7}';
const YELLOW_OPEN: char = '\u{e}';
const CHIP_OPEN: char = '\u{f}';
const CLOSE: char = '\u{10}';
const ACCENT_OPEN: char = '\u{11}';

/// Where an action row's chip starts.
const ROW_INDENT: &str = "   ";

fn tint(open: char, text: &str) -> String {
    format!("{open}{text}{CLOSE}")
}

fn dim(text: &str) -> String {
    format!("{DIM_OPEN}{text}{DIM_CLOSE}")
}

/// The key as the footer draws its chips: ` u ` on a button colour, `[u]` without colour.
fn chip(key: char, color: bool) -> String {
    if color {
        tint(CHIP_OPEN, &format!(" {key} "))
    } else {
        format!("[{key}]")
    }
}

/// Facts joined by a dim dot; each piece is already dim or tinted.
fn facts(pieces: &[String], g: &Glyphs) -> String {
    pieces.join(&dim(&format!(" {} ", g.dot)))
}

/// `0.9.0 · up to date · self-update notify` from the module's `self-update notify, up to date`.
fn launcher_facts(p: &Page, g: &Glyphs) -> String {
    facts(&launcher_pieces(p), g)
}

/// The version, the state and the mode, each tinted.
fn launcher_pieces(p: &Page) -> Vec<String> {
    let words = strip_ctl(&p.launcher);
    let mut out = vec![dim(env!("CARGO_PKG_VERSION"))];
    match words.split_once(", ") {
        Some((mode, state)) => {
            let open = if state.contains("available") { YELLOW_OPEN } else { GREEN_OPEN };
            out.push(tint(open, state));
            out.push(dim(mode));
        }
        None => out.push(dim(if words.is_empty() { "self-update off" } else { &words })),
    }
    out
}

/// The row's right column: the live state of the last action, else the selected row's hint.
fn row_state(p: &Page, e: &Entry, selected: bool) -> String {
    let first = |s: &str| s.lines().map(str::trim).find(|l| !l.is_empty()).map(strip_ctl);
    if e.name == p.last {
        match &p.phase {
            Phase::Idle => {}
            Phase::Armed => return first(&p.status).unwrap_or_else(|| "press again".into()),
            Phase::Running(step) => return strip_ctl(step),
            Phase::Done => return first(&clean(&p.status)).unwrap_or_else(|| "done".into()),
            Phase::Failed(why) => return format!("failed: {}", strip_ctl(why)),
        }
    }
    if selected { strip_ctl(&e.about) } else { String::new() }
}

/// The status without the markers a report may carry.
fn clean(status: &str) -> String {
    status.replace(['\u{1}', '\u{2}', '\u{4}', '\u{5}'], "")
}

fn state_tint(p: &Page, e: &Entry) -> fn(&str) -> String {
    if e.name != p.last {
        return dim;
    }
    match p.phase {
        Phase::Idle => dim,
        Phase::Armed | Phase::Running(_) => |s| tint(YELLOW_OPEN, s),
        Phase::Done => |s| tint(GREEN_OPEN, s),
        Phase::Failed(_) => |s| tint(RED_OPEN, s),
    }
}

/// The versions and warnings column beside the last run, once the page has room for three
/// columns: a config warning's `fix it in <path>` line with its indent.
const FACTS_WIDTH: usize = 66;
/// The last run's output needs at least this much to stand as its own column.
const RUN_MIN_WIDTH: usize = 60;

/// How the action rows are laid out when room is short.
#[derive(Debug, Clone, Copy)]
struct Lay {
    /// The last run's state goes on its own line under its row.
    run_under: bool,
    /// A blank row between the groups.
    gaps: bool,
}

/// Where an action's name starts: the indent, the three-cell chip and a space; a state moved
/// under its row starts here too.
const NAME_COLUMN: usize = 7;

/// One action row: chip and name on the left, the state or hint ending at `edge`. A state that
/// does not fit beside the name, and the last run's state when `under` (stacked: the last run
/// panel may not fit), goes on its own line under the row instead of being cut. The selected row
/// is a fill band; a hovered one paints its chip and name.
fn action_row(p: &Page, i: usize, e: &Entry, edge: usize, style: Style, under: bool) -> Vec<String> {
    let (color, ascii) = (style.color, style.ascii);
    let selected = i == p.selected;
    let name = strip_ctl(&e.name);
    let left_plain = format!(
        "{ROW_INDENT}{} {name}",
        if color { format!(" {} ", e.key) } else { format!("[{}]", e.key) }
    );
    let room = edge.saturating_sub(display_width(&left_plain) + 2);
    let state = row_state(p, e, selected);
    let hovered = i32::try_from(i).is_ok_and(|ii| ii == p.hover_row);
    let body = format!("{} {name}", chip(e.key, color));
    let body = if hovered { add_hover_span(&body) } else { body };
    let row = format!("{ROW_INDENT}{body}");
    if state.is_empty() {
        return vec![row];
    }
    let ran = e.name == p.last && p.phase != Phase::Idle;
    if display_width(&state) > room || (under && ran) {
        let line = limit_line(&state, edge.saturating_sub(NAME_COLUMN), ascii);
        return vec![row, format!("{}{}", " ".repeat(NAME_COLUMN), state_tint(p, e)(&line))];
    }
    let pad = edge.saturating_sub(display_width(&left_plain) + display_width(&state));
    vec![format!("{row}{}{}", " ".repeat(pad), state_tint(p, e)(&state))]
}

/// `pieces` joined by a dim dot, the last ones left out until the line fits `room` cells.
fn fit_facts(mut pieces: Vec<String>, room: usize, g: &Glyphs) -> String {
    while pieces.len() > 1 && display_width(&strip_ctl(&facts(&pieces, g))) > room {
        pieces.pop();
    }
    facts(&pieces, g)
}

fn footer(g: &Glyphs, width: usize, color: bool) -> Footer {
    new_hint_footer(
        &[
            Hint::plain("w/s", "move").kept(),
            Hint::key("enter", "run", "Enter"),
            Hint::key("esc", "back", "Escape"),
        ],
        g,
        width,
        !color,
    )
}

/// Markers to colour, or out when colour is off; a cut marker never bleeds past its line.
fn paint(line: &str, color: bool, g: &Glyphs) -> String {
    let marks = [GREEN_OPEN, RED_OPEN, YELLOW_OPEN, CHIP_OPEN, CLOSE, ACCENT_OPEN];
    if !color {
        return theme::add_select_band_color(&line.replace(marks, ""), false);
    }
    let sparkle = format!("{}{}{}", theme::ACCENT, g.sparkle, theme::RESET);
    let mut out = line.replace(g.sparkle, &sparkle);
    if out.contains(marks) {
        let chip = format!("{}{}", theme::BUTTON_BG, theme::BUTTON_FG);
        out = out
            .replace(GREEN_OPEN, theme::GREEN)
            .replace(RED_OPEN, theme::RED)
            .replace(YELLOW_OPEN, theme::YELLOW)
            .replace(CHIP_OPEN, &chip)
            .replace(ACCENT_OPEN, &format!("{}{}", theme::BOLD, cockpit::ACCENT))
            .replace(CLOSE, theme::RESET);
        out.push_str(theme::RESET);
    }
    theme::add_select_band_color(&out, true)
}

/// The page in the cockpit frame: the actions in groups in the focused panel; beside it (under it
/// below `cockpit::TWO_COLUMNS` columns) the last run, the versions and the warnings. A panel with
/// nothing to show is not drawn; a short terminal cuts the panels, never the footer.
pub(crate) fn maintenance_frame(p: &Page, size: (usize, usize), style: Style) -> Frame {
    let (width, height) = size;
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return Frame {
            lines: too_small_frame(width, height, style.ascii),
            map: RowMap::default(),
        };
    }
    let g = glyphs(style.ascii);
    let fw = frame_width(width);
    let wide = width >= cockpit::TWO_COLUMNS;
    let lw = if wide { cockpit::MAINTENANCE_LEFT_WIDTH } else { fw };
    let rw = fw - lw;
    let foot = footer(&g, fw, style.color);
    let page_rows = height.saturating_sub(1 + foot.lines.len());
    let right_inner = if wide { rw - 2 } else { fw - 2 };
    let versions = version_lines(p, &g);
    let warnings = warning_lines(p, &g, right_inner);
    let warn_h = if warnings.is_empty() { 0 } else { warnings.len() + 2 };
    let ran = !p.last.is_empty() && p.phase != Phase::Idle;
    let (actions, rows) = fitted_actions(p, lw - 2, style, (!wide).then_some(&g), (page_rows, warn_h));
    let painter = |l: &str, e: bool| paint(l, e, &g);
    let selected = rows
        .iter()
        .position(|r| r.index == p.selected)
        .and_then(|i| rows.get(i))
        .map(|r| usize::try_from(r.y).unwrap_or(1) - 1);
    let left_h = if wide {
        let run_need = if ran { 4 } else { 3 };
        (actions.len() + 2).max(versions.len() + 2 + warn_h + run_need).min(page_rows)
    } else {
        (actions.len() + 2).min(page_rows)
    };
    let main = Panel {
        title: cockpit::identity(&g),
        page: "maintenance".into(),
        right: env!("CARGO_PKG_VERSION").into(),
        focus: Focus::Focused,
        lines: actions,
        width: lw,
        height: left_h,
        painter: &painter,
        selected,
    };
    let mut lines = cockpit::render(&main, style);
    let mut map = RowMap {
        rows: rows
            .into_iter()
            .filter(|r| usize::try_from(r.y).is_ok_and(|y| y + 1 < left_h))
            .collect(),
        ..RowMap::default()
    };
    let panel = |title: &str, body: Vec<String>, w: usize, h: usize| {
        let mut pn = Panel::plain(title, body, w, h);
        pn.painter = &painter;
        cockpit::render(&pn, style)
    };
    let run_title = if ran {
        format!("{} {} last run", strip_ctl(&p.last), g.dot)
    } else {
        "last run".to_owned()
    };
    if wide && rw >= FACTS_WIDTH + RUN_MIN_WIDTH {
        // Room for three columns: the facts keep their width, the last run's output takes the rest.
        let run_w = rw - FACTS_WIDTH;
        let run = panel(&run_title, run_lines(p, run_w - 2, left_h - 2, style.ascii), run_w, left_h);
        let mut facts = panel("versions", versions, FACTS_WIDTH, 6);
        let warnings = warning_lines(p, &g, FACTS_WIDTH - 2);
        if warn_h > 0 {
            facts.extend(panel("warnings", warnings, FACTS_WIDTH, warn_h));
        }
        facts.truncate(left_h);
        lines = cockpit::beside(lines, lw, &cockpit::beside(run, run_w, &facts));
    } else if wide {
        let run_h = left_h.saturating_sub(versions.len() + 2 + warn_h).max(3);
        let mut right = panel(&run_title, run_lines(p, rw - 2, run_h - 2, style.ascii), rw, run_h);
        right.extend(panel("versions", versions, rw, 6));
        if warn_h > 0 {
            right.extend(panel("warnings", warnings, rw, warn_h));
        }
        right.truncate(page_rows.max(left_h));
        lines = cockpit::beside(lines, lw, &right);
    } else {
        let mut room = page_rows.saturating_sub(lines.len());
        if warn_h > 0 && room >= 3 {
            let h = warn_h.min(room);
            lines.extend(panel("warnings", warnings, fw, h));
            room -= h;
        }
        if ran && room >= 3 {
            let h = room.min(6);
            lines.extend(panel(&run_title, run_lines(p, fw - 2, h - 2, style.ascii), fw, h));
        }
    }
    let light = FooterLight {
        hover: footer_hover(&foot, p.hover),
        selected: Vec::new(),
    };
    let lines = append_footer(lines, &foot, fw, style, &light, &mut map);
    Frame { lines, map }
}

/// The action rows under their group titles, a blank row between groups; their row hits sit at
/// the page row each is drawn on (the panel's top border is row 0).
/// `inline`: stacked, the Claude Code and claude-auto titles carry their facts (no versions panel).
/// The action rows laid out for `rows` page rows and a warnings panel of `warn_h`: stacked with
/// no room left for the last run panel, its state goes under the action that ran; a short window
/// drops the blank rows between the groups before it cuts an action.
fn fitted_actions(
    p: &Page,
    inner: usize,
    style: Style,
    inline: Option<&Glyphs>,
    (rows, warn_h): (usize, usize),
) -> (Vec<String>, Vec<RowHit>) {
    let ran = !p.last.is_empty() && p.phase != Phase::Idle;
    let mut lay = Lay {
        run_under: false,
        gaps: true,
    };
    let mut out = action_lines(p, inner, style, inline, lay);
    if inline.is_some() && ran && rows.saturating_sub(out.0.len() + 2 + warn_h) < 3 {
        lay.run_under = true;
        out = action_lines(p, inner, style, inline, lay);
    }
    if out.0.len() + 2 > rows {
        lay.gaps = false;
        out = action_lines(p, inner, style, inline, lay);
    }
    out
}

fn action_lines(p: &Page, inner: usize, style: Style, inline: Option<&Glyphs>, lay: Lay) -> (Vec<String>, Vec<RowHit>) {
    let mut body: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    let mut group: Option<Group> = None;
    for (i, e) in p.entries.iter().enumerate() {
        if group != Some(e.group) {
            if group.is_some() && lay.gaps {
                body.push(String::new());
            }
            group = Some(e.group);
            // Stacked, the facts the versions panel would hold ride on the titles, cut to fit.
            let room = inner.saturating_sub(4 + e.group.title().len() + 2);
            let facts = match (inline, e.group) {
                (Some(g), Group::ClaudeCode) => {
                    let mut pieces = vec![installed_text(p, g)];
                    pieces.extend(disk_pieces(p));
                    fit_facts(pieces, room, g)
                }
                (Some(g), Group::ClaudeAuto) => fit_facts(launcher_pieces(p), room, g),
                _ => String::new(),
            };
            let title = tint(ACCENT_OPEN, e.group.title());
            body.push(if facts.is_empty() {
                format!("  {title}")
            } else {
                format!("  {title}  {facts}")
            });
        }
        rows.push(RowHit {
            index: i,
            name: e.key.to_string(),
            y: i32::try_from(1 + body.len()).unwrap_or(0),
            cells: Vec::new(),
        });
        body.extend(action_row(p, i, e, inner.saturating_sub(2), style, lay.run_under));
    }
    (body, rows)
}

/// The installed build against the newest: `2.1.230 · newest`, `2.1.226 · update available 2.1.230`.
fn installed_text(p: &Page, g: &Glyphs) -> String {
    let installed = p
        .installed
        .clone()
        .or_else(|| p.info.matches.then(|| p.info.newest_version.clone()).flatten());
    match (&installed, &newest_of(p)) {
        (Some(i), Some(n)) if i == n => facts(&[strip_ctl(i), tint(GREEN_OPEN, "newest")], g),
        (Some(i), Some(n)) => facts(&[strip_ctl(i), tint(YELLOW_OPEN, &format!("update available {}", strip_ctl(n)))], g),
        (Some(i), None) => strip_ctl(i),
        (None, _) => tint(YELLOW_OPEN, "version unknown"),
    }
}

fn newest_of(p: &Page) -> Option<String> {
    p.newest.clone().or_else(|| p.info.newest_version.clone())
}

fn disk_pieces(p: &Page) -> Vec<String> {
    vec![
        format!("{} builds", p.info.version_count),
        format!("{} GB", gib(p.info.versions_bytes)),
    ]
}

/// `3 builds · 0.8 GB`.
fn disk_text(p: &Page, g: &Glyphs) -> String {
    facts(&disk_pieces(p), g)
}

/// Installed, newest, the builds on disk and claude-auto, each after a dim label.
fn version_lines(p: &Page, g: &Glyphs) -> Vec<String> {
    let line = |label: &str, value: String| format!("  {}{value}", dim(&format!("{label:<14}")));
    vec![
        line("installed", installed_text(p, g)),
        line("newest", newest_of(p).as_deref().map_or_else(|| dim("unknown"), strip_ctl)),
        line("on disk", disk_text(p, g)),
        line("claude-auto", launcher_facts(p, g)),
    ]
}

/// One line per warning, its label coloured; a config warning names the file to fix it in.
fn warning_lines(p: &Page, g: &Glyphs, inner: usize) -> Vec<String> {
    let mut out = Vec::new();
    for w in &p.warnings {
        let w = strip_ctl(w);
        let label = if w.contains("not found") || w.contains("failed") {
            tint(RED_OPEN, "error  ")
        } else {
            tint(YELLOW_OPEN, "warning")
        };
        out.push(format!(
            "  {label}  {} {}",
            g.warn,
            limit_line(&w, inner.saturating_sub(15), g.on == '*')
        ));
        if w.starts_with("config:") && !p.config_path.is_empty() {
            out.push(format!("           {}", dim(&format!("fix it in {}", strip_ctl(&p.config_path)))));
        }
    }
    out
}

/// The last run: the tail of its report that fits, then where it stands.
fn run_lines(p: &Page, inner: usize, rows: usize, ascii: bool) -> Vec<String> {
    let state = match &p.phase {
        Phase::Idle => return vec![format!("  {}", dim("nothing run yet"))],
        Phase::Armed => tint(YELLOW_OPEN, "press again to run"),
        Phase::Running(step) => tint(YELLOW_OPEN, &strip_ctl(step)),
        Phase::Done => tint(GREEN_OPEN, "done"),
        Phase::Failed(why) => tint(RED_OPEN, &format!("failed: {}", strip_ctl(why))),
    };
    let report: Vec<String> = clean(&p.status)
        .lines()
        .map(|l| limit_line(&strip_ctl(l.trim_end()), inner.saturating_sub(3), ascii))
        .filter(|l| !l.trim().is_empty())
        .collect();
    let keep = rows.saturating_sub(1);
    let mut out: Vec<String> = report
        .iter()
        .skip(report.len().saturating_sub(keep))
        .map(|l| format!("  {l}"))
        .collect();
    out.push(format!("  {state}"));
    out
}
