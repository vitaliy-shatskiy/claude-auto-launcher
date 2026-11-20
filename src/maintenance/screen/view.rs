//! The maintenance page, pure: the header box, then one column of groups. A group's header names
//! it in the accent colour with its facts inline (dim, the status word coloured); its actions sit
//! indented under it, chip and name on the left, the hint or the live state right-aligned.

use crate::frame::{
    BODY_TOP, Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, RowHit, RowMap, Style, complete_frame, footer_hover, frame_width,
    header_page, new_hint_footer, right_edge, too_small_frame,
};
use crate::layout::{display_width, limit_line};
use crate::theme::{self, DIM_CLOSE, DIM_OPEN, Glyphs, SELECT_CLOSE, SELECT_OPEN, add_hover_span, glyphs, strip_ctl};

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

/// The group title column.
const GROUP_W: usize = 15;
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

fn claude_code_facts(p: &Page, g: &Glyphs) -> String {
    let installed = p
        .installed
        .clone()
        .or_else(|| p.info.matches.then(|| p.info.newest_version.clone()).flatten());
    let newest = p.newest.clone().or_else(|| p.info.newest_version.clone());
    let mut out = match (&installed, &newest) {
        (Some(i), Some(n)) if i == n => vec![dim(&strip_ctl(i)), tint(GREEN_OPEN, "newest")],
        (Some(i), Some(n)) => vec![dim(&strip_ctl(i)), tint(YELLOW_OPEN, &format!("update available {}", strip_ctl(n)))],
        (Some(i), None) => vec![dim(&strip_ctl(i))],
        (None, _) => vec![tint(YELLOW_OPEN, "version unknown")],
    };
    out.push(dim(&format!("{} builds", p.info.version_count)));
    out.push(dim(&format!("{} GB", gib(p.info.versions_bytes))));
    facts(&out, g)
}

/// `0.9.0 · up to date · self-update notify` from the module's `self-update notify, up to date`.
fn launcher_facts(p: &Page, g: &Glyphs) -> String {
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
    facts(&out, g)
}

fn group_line(title: &str, facts: &str) -> String {
    if facts.is_empty() {
        return format!(" {}", tint(ACCENT_OPEN, title));
    }
    let pad = " ".repeat(GROUP_W.saturating_sub(display_width(title)));
    format!(" {}{pad}{facts}", tint(ACCENT_OPEN, title))
}

fn warning_lines(p: &Page, g: &Glyphs) -> Vec<String> {
    if p.warnings.is_empty() {
        return vec![group_line("Warnings", &tint(GREEN_OPEN, "none"))];
    }
    let lead = " ".repeat(GROUP_W + 1);
    let mut out = Vec::new();
    for (i, w) in p.warnings.iter().enumerate() {
        let text = tint(YELLOW_OPEN, &format!("{} {}", g.warn, strip_ctl(w)));
        out.push(if i == 0 {
            group_line("Warnings", &text)
        } else {
            format!("{lead}{text}")
        });
        if w.starts_with("config:") && !p.config_path.is_empty() {
            out.push(format!("{lead}  {}", dim(&format!("fix it in {}", strip_ctl(&p.config_path)))));
        }
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

/// One action row: chip and name on the left, the state or hint ending at `edge`. The selected
/// row is a fill band; a hovered one paints its chip and name.
fn action_row(p: &Page, i: usize, e: &Entry, edge: usize, color: bool, ascii: bool) -> String {
    let selected = i == p.selected;
    let name = strip_ctl(&e.name);
    let left_plain = format!(
        "{ROW_INDENT}{} {name}",
        if color { format!(" {} ", e.key) } else { format!("[{}]", e.key) }
    );
    let room = edge.saturating_sub(display_width(&left_plain) + 2);
    let state = limit_line(&row_state(p, e, selected), room, ascii);
    let hovered = i32::try_from(i).is_ok_and(|ii| ii == p.hover_row);
    let body = format!("{} {name}", chip(e.key, color));
    let body = if hovered { add_hover_span(&body) } else { body };
    let pad = edge.saturating_sub(display_width(&left_plain) + display_width(&state));
    let line = if state.is_empty() {
        format!("{ROW_INDENT}{body}")
    } else {
        format!("{ROW_INDENT}{body}{}{}", " ".repeat(pad), state_tint(p, e)(&state))
    };
    if selected {
        // The band spans to the right edge so the fill reads as a row.
        let fill = " ".repeat(edge.saturating_sub(display_width(&line)));
        format!("{SELECT_OPEN}{line}{fill}{SELECT_CLOSE}")
    } else {
        line
    }
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
            .replace(ACCENT_OPEN, &format!("{}{}", theme::BOLD, theme::ACCENT))
            .replace(CLOSE, theme::RESET);
        out.push_str(theme::RESET);
    }
    theme::add_select_band_color(&out, true)
}

/// The page at content height: header box, the groups one blank row apart, rule and footer.
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
    let edge = right_edge(width);
    let foot = footer(&g, fw, style.color);
    let mut body: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    let mut group: Option<Group> = None;
    for (i, e) in p.entries.iter().enumerate() {
        if group != Some(e.group) {
            if group.is_some() {
                body.push(String::new());
            }
            group = Some(e.group);
            let facts = match e.group {
                Group::ClaudeCode => claude_code_facts(p, &g),
                Group::ClaudeAuto => launcher_facts(p, &g),
                Group::Checks | Group::Launcher | Group::Custom => String::new(),
            };
            body.push(group_line(e.group.title(), &facts));
        }
        rows.push(RowHit {
            index: i,
            name: e.key.to_string(),
            y: i32::try_from(BODY_TOP + body.len()).unwrap_or(0),
            cells: Vec::new(),
        });
        body.push(action_row(p, i, e, edge, style.color, style.ascii));
    }
    if !body.is_empty() {
        body.push(String::new());
    }
    body.extend(warning_lines(p, &g));
    // A short terminal cuts the body: the header, the rule, the footer and the headroom row stay.
    let room = height.saturating_sub(BODY_TOP + 3 + foot.lines.len());
    if body.len() > room {
        body.truncate(room);
        rows.retain(|r| usize::try_from(r.y).is_ok_and(|y| y < BODY_TOP + room));
    }
    let title = format!("{} claude-auto {} maintenance", g.sparkle, g.dot);
    let lines = header_page(body, width, &title, &[], style);
    let mut map = RowMap { rows, ..RowMap::default() };
    let light = FooterLight {
        hover: footer_hover(&foot, p.hover),
        selected: Vec::new(),
    };
    let painter = |l: &str, e: bool| paint(l, e, &g);
    let lines = complete_frame(&lines, &foot, fw, style, &painter, &light, &mut map);
    Frame { lines, map }
}
