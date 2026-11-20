//! The maintenance page, pure: the action list on the left (key chip and name; the selected row
//! adds a short dim hint where its name cannot carry it), the facts on the right as a label/value
//! grid (Claude Code, claude-auto, warnings, the last action's state).

use crate::frame::{
    Footer, FooterLight, Frame, Hint, MIN_HEIGHT, MIN_WIDTH, PAD, RowHit, RowMap, Style, TWO_PANE_WIDTH, complete_frame, footer_hover,
    frame_width, new_hint_footer, page, too_small_frame,
};
use crate::layout::{display_width, join_panes, split_output_lines};
use crate::theme::{self, DIM_CLOSE, DIM_OPEN, Glyphs, glyphs, strip_ctl};

use super::super::install::InstallInfo;
use super::gib;

/// One action row: its key, its name and the hint the selected row shows ('' = none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) key: char,
    pub(crate) name: String,
    pub(crate) about: String,
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
    /// claude-auto's self-update mode and state, in words.
    pub(crate) launcher: String,
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
}

// Colour markers, resolved by the page's painter after layout (zero cells wide, stripped plain).
const GREEN_OPEN: char = '\u{6}';
const RED_OPEN: char = '\u{7}';
const YELLOW_OPEN: char = '\u{e}';
const CHIP_OPEN: char = '\u{f}';
const CLOSE: char = '\u{10}';

/// The label column of the facts grid.
const LABEL: usize = 13;

fn tint(open: char, text: &str) -> String {
    format!("{open}{text}{CLOSE}")
}

/// The key as the footer draws its chips: ` u ` on a button colour, `[u]` without colour.
fn chip(key: char, color: bool) -> String {
    if color {
        tint(CHIP_OPEN, &format!(" {key} "))
    } else {
        format!("[{key}]")
    }
}

/// One grid row: the label column, then the value.
fn row(label: &str, value: &str) -> String {
    format!("{label:<LABEL$}{value}")
}

/// The action list: chip and name in their own columns; the selected row carries the cursor and
/// its hint.
fn action_rows(p: &Page, g: &Glyphs, color: bool) -> Vec<String> {
    let name_w = p.entries.iter().map(|e| display_width(&e.name)).max().unwrap_or(0);
    p.entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let name = strip_ctl(&e.name);
            let line = format!(" {} {} {name}", if i == p.selected { g.cursor } else { ' ' }, chip(e.key, color));
            if i == p.selected && !e.about.is_empty() {
                let pad = " ".repeat(name_w.saturating_sub(display_width(&name)) + 2);
                format!("{line}{pad}{DIM_OPEN}{}{DIM_CLOSE}", strip_ctl(&e.about))
            } else {
                line
            }
        })
        .collect()
}

fn claude_code_rows(p: &Page, g: &Glyphs) -> Vec<String> {
    let installed = p
        .installed
        .clone()
        .or_else(|| p.info.matches.then(|| p.info.newest_version.clone()).flatten());
    let newest = p.newest.clone().or_else(|| p.info.newest_version.clone());
    let state = match (&installed, &newest) {
        (Some(i), Some(n)) if i == n => format!("{i} {} newest", g.dot),
        (Some(i), Some(n)) => format!("{i} {} {}", g.dot, tint(YELLOW_OPEN, &format!("update available {n}"))),
        (Some(i), None) => i.clone(),
        (None, _) => "version unknown".into(),
    };
    vec![
        row("Claude Code", &strip_ctl(&state)),
        row(
            "",
            &format!(
                "{} builds on disk {} {} GB",
                p.info.version_count,
                g.dot,
                gib(p.info.versions_bytes)
            ),
        ),
    ]
}

fn launcher_row(p: &Page, g: &Glyphs) -> String {
    let update = if p.launcher.is_empty() { "self-update off" } else { &p.launcher };
    row(
        "claude-auto",
        &format!("{} {} {}", env!("CARGO_PKG_VERSION"), g.dot, strip_ctl(update)),
    )
}

fn warning_rows(p: &Page, g: &Glyphs) -> Vec<String> {
    if p.warnings.is_empty() {
        return vec![row("Warnings", "none")];
    }
    let mut out = Vec::new();
    for (i, w) in p.warnings.iter().enumerate() {
        let label = if i == 0 { "Warnings" } else { "" };
        out.push(row(label, &format!("{} {}", tint(YELLOW_OPEN, &g.warn.to_string()), strip_ctl(w))));
        if w.starts_with("config:") && !p.config_path.is_empty() {
            out.push(row("", &format!("  fix it in {}", strip_ctl(&p.config_path))));
        }
    }
    out
}

/// The last action: its name and state on one grid row, then what it reported.
fn last_rows(p: &Page, g: &Glyphs, width: usize, room: usize) -> Vec<String> {
    let state = match &p.phase {
        Phase::Idle => return Vec::new(),
        Phase::Armed => tint(YELLOW_OPEN, "waiting for the second press"),
        Phase::Running(step) => tint(YELLOW_OPEN, &strip_ctl(step)),
        Phase::Done => tint(GREEN_OPEN, "done"),
        Phase::Failed(why) => tint(RED_OPEN, &format!("failed: {}", strip_ctl(why))),
    };
    let mut out = vec![row("Last action", &format!("{} {} {state}", strip_ctl(&p.last), g.dot))];
    let clean = p.status.replace(['\u{1}', '\u{2}', '\u{4}', '\u{5}'], "");
    let lines = split_output_lines(&clean, width.saturating_sub(LABEL), room.saturating_sub(1).max(1));
    out.extend(lines.iter().map(|l| row("", l)));
    out
}

/// The facts grid, sections one blank row apart; `width` is the panel's own.
fn panel(p: &Page, g: &Glyphs, width: usize, room: usize) -> Vec<String> {
    let mut out = claude_code_rows(p, g);
    out.push(launcher_row(p, g));
    out.extend(warning_rows(p, g));
    let last = last_rows(p, g, width, room.saturating_sub(out.len() + 1));
    if !last.is_empty() {
        out.push(String::new());
        out.extend(last);
    }
    out
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
fn paint(line: &str, color: bool) -> String {
    let marks = [GREEN_OPEN, RED_OPEN, YELLOW_OPEN, CHIP_OPEN, CLOSE];
    if !line.contains(marks) {
        return line.to_owned();
    }
    if !color {
        return line.replace(marks, "");
    }
    let chip = format!("{}{}", theme::BUTTON_BG, theme::BUTTON_FG);
    let out = line
        .replace(GREEN_OPEN, theme::GREEN)
        .replace(RED_OPEN, theme::RED)
        .replace(YELLOW_OPEN, theme::YELLOW)
        .replace(CHIP_OPEN, &chip)
        .replace(CLOSE, theme::RESET);
    format!("{out}{}", theme::RESET)
}

/// The page: list and facts side by side from `TWO_PANE_WIDTH` columns, the facts under the
/// list below it.
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
    let foot = footer(&g, fw, style.color);
    // Borders, footer and the headroom row.
    let room = height.saturating_sub(3 + foot.lines.len());
    let list = action_rows(p, &g, style.color);
    let inner = fw.saturating_sub(2);
    let body = if width >= TWO_PANE_WIDTH {
        // Wide enough for any row's hint, so the divider never moves with the selection.
        let name_w = p.entries.iter().map(|e| display_width(&e.name)).max().unwrap_or(0);
        let hint_w = p.entries.iter().map(|e| display_width(&e.about)).max().unwrap_or(0);
        let left_w = (7 + name_w + 2 + hint_w + 2).min(inner * 3 / 5);
        let right_w = inner.saturating_sub(left_w + 1);
        let facts: Vec<String> = panel(p, &g, right_w.saturating_sub(2), room)
            .iter()
            .map(|l| format!(" {l}"))
            .collect();
        let mut left = list;
        left.resize(room.max(facts.len()), String::new());
        join_panes(&left, &facts, left_w, right_w, &g, style.ascii)
    } else {
        let (room_all, list_len) = (room, list.len());
        let mut body = list;
        body.push(String::new());
        let room = room.saturating_sub(body.len());
        body.extend(
            panel(p, &g, inner.saturating_sub(PAD.len()), room)
                .iter()
                .map(|l| format!("{PAD}{l}")),
        );
        // A short terminal keeps the list whole and cuts the facts under it.
        body.truncate(room_all.max(list_len));
        body
    };
    let title = format!("maintenance {} {} actions", g.h, p.entries.len());
    let lines = page(body, size, foot.lines.len(), &title, "", style);
    let mut map = RowMap {
        // 1 = the page's top border: the list starts on the first body row.
        rows: p
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| RowHit {
                index: i,
                name: e.key.to_string(),
                y: i32::try_from(1 + i).unwrap_or(0),
                cells: Vec::new(),
            })
            .collect(),
        ..RowMap::default()
    };
    let light = FooterLight {
        hover: footer_hover(&foot, p.hover),
        selected: Vec::new(),
    };
    let painter = |l: &str, e: bool| paint(&crate::list::add_picker_color(l, e, &g), e);
    let lines = complete_frame(&lines, &foot, fw, style, &painter, &light, &mut map);
    Frame { lines, map }
}
