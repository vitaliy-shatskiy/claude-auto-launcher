//! The lines of the launch page's side panels: limits, running sessions, Claude Code's state.

use super::RunningSession;
use crate::cockpit;
use crate::layout::{self, display_width};
use crate::theme::{self, Glyphs};
use crate::types::{LimitRecord, VersionInfo};

/// The limit lines of the selected account: one per limit, label dim, percentage bold, coloured
/// and right-aligned 2 cells from the border; `None` without a record.
pub(super) fn limit_lines(l: Option<&LimitRecord>, inner: usize, one_line: bool) -> (Vec<String>, String) {
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
    // Values follow a label column (the longest label and two cells), as the selected panel's facts.
    let shown: Vec<(&String, i64)> = shown.collect();
    let label_w = shown
        .iter()
        .map(|(k, _)| display_width(k))
        .max()
        .unwrap_or(0)
        .min(inner.saturating_sub(10));
    let lines = shown
        .iter()
        .map(|(k, p)| {
            let label = layout::limit_line(k, label_w, false);
            let pad = " ".repeat(label_w.saturating_sub(display_width(&label)) + 2);
            format!("  {}{pad}{}", cockpit::dim(&label), theme::tone(tone_of(*p), &format!("{p:>3}%")))
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
pub(super) fn pin_last(mut lines: Vec<String>, last: String, height: usize) -> Vec<String> {
    if last.is_empty() {
        return lines;
    }
    lines.resize(height.saturating_sub(3).max(lines.len()), String::new());
    lines.push(last);
    lines
}

/// The projects panel while the registry is still being read.
pub(super) fn loading_lines(ascii: bool) -> Vec<String> {
    vec![format!(
        "  {}",
        cockpit::dim(&format!("loading projects{}", layout::ellipsis(ascii)))
    )]
}

pub(super) fn running_lines(running: &[RunningSession], inner: usize, rows: usize, g: &Glyphs) -> Vec<String> {
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
pub(super) fn claude_code_lines(version: &VersionInfo, warnings: usize, mcp: &str, g: &Glyphs) -> Vec<String> {
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
