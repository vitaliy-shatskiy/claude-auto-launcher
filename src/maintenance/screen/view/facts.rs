//! The maintenance page's fact panels: the installed and newest builds, disk, versions, warnings,
//! the selected action's details and the last run's output.

use super::super::gib;
use super::{ACCENT_OPEN, GREEN_OPEN, Page, Phase, RED_OPEN, YELLOW_OPEN, clean, dim, facts, launcher_facts, tint};
use crate::layout::limit_line;
use crate::theme::{Glyphs, glyphs, strip_ctl};

/// The installed build against the newest: `2.1.230 · newest`, `2.1.226 · update available 2.1.230`.
pub(super) fn installed_text(p: &Page, g: &Glyphs) -> String {
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

pub(super) fn disk_pieces(p: &Page) -> Vec<String> {
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
pub(super) fn version_lines(p: &Page, g: &Glyphs) -> Vec<String> {
    let line = |label: &str, value: String| format!("  {}{value}", dim(&format!("{label:<14}")));
    vec![
        line("installed", installed_text(p, g)),
        line("newest", newest_of(p).as_deref().map_or_else(|| dim("unknown"), strip_ctl)),
        line("on disk", disk_text(p, g)),
        line("claude-auto", launcher_facts(p, g)),
    ]
}

/// One line per warning, its label coloured; a config warning names the file to fix it in.
pub(super) fn warning_lines(p: &Page, g: &Glyphs, inner: usize) -> Vec<String> {
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

/// What each built-in row does, for the selected-action panel beside the rows.
const DETAILS: [(char, &str); 6] = [
    ('u', "downloads the newest Claude Code and switches to it"),
    ('r', "moves the running claude.exe aside, then updates"),
    ('p', "deletes every build but the 2 newest and the running one"),
    ('d', "runs claude doctor and shows its report"),
    ('m', "checks each MCP server's connection"),
    ('l', "show, hide and order the launch rows; pin a hidden row's value"),
];

/// Beside the rows on a wide page: the selected action - what it does, how to run it - and its
/// last run, or the last run of another action under it.
pub(super) fn selected_action(p: &Page, inner: usize, rows: usize, ascii: bool) -> (String, Vec<String>) {
    let Some(e) = p.entries.get(p.selected) else {
        return ("last run".to_owned(), run_lines(p, inner, rows, ascii));
    };
    let what = DETAILS.iter().find(|(k, _)| *k == e.key).map_or("a configured action", |(_, d)| *d);
    let how = vec![
        format!("  {}", limit_line(what, inner.saturating_sub(3), ascii)),
        String::new(),
        format!(
            "  {}  or  {}  runs it",
            tint(ACCENT_OPEN, "enter"),
            tint(ACCENT_OPEN, &e.key.to_string())
        ),
    ];
    let name = strip_ctl(&e.name);
    if p.phase == Phase::Idle {
        return (name, how);
    }
    // A run's report and state come first; what the action does follows where rows are left.
    let mine = strip_ctl(&p.last) == name;
    let mut out = Vec::new();
    if !mine {
        out.push(format!("  {}", dim(&format!("last run  {}", strip_ctl(&p.last)))));
    }
    out.extend(run_lines(p, inner, rows.saturating_sub(out.len()), ascii));
    if out.len() + 1 + how.len() <= rows {
        out.push(String::new());
        out.extend(how);
    }
    let title = if mine {
        format!("{name} {} last run", glyphs(ascii).dot)
    } else {
        name
    };
    (title, out)
}

/// The last run: the tail of its report that fits, then where it stands.
pub(super) fn run_lines(p: &Page, inner: usize, rows: usize, ascii: bool) -> Vec<String> {
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
