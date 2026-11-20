//! Row markers of the project frame: filter-match spans and the dim spans (a missing pin's
//! whole row, a pin's mark). Pure; laid out before the markers are resolved.

use crate::theme;

/// Filter-match markers, resolved after layout like the dim ones (C0, zero cells wide).
const MATCH_OPEN: char = '\u{6}';
const MATCH_CLOSE: char = '\u{e}';
/// Underline and the accent, closed without a Reset so a dim tail or a hover band stays on.
pub(super) const MATCH_ON: &str = "\x1b[4m\x1b[38;5;209m";
const MATCH_OFF: &str = "\x1b[24m\x1b[39m";

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Marks every case-insensitive occurrence of a filter word among the visible characters
/// `from..to` of a finished row (control characters are markers and skipped).
pub(super) fn mark_matches(row: &str, words: &[String], from: usize, to: usize) -> String {
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
pub(super) fn add_match_color(line: &str, enabled: bool) -> String {
    if enabled {
        line.replace(MATCH_OPEN, MATCH_ON).replace(MATCH_CLOSE, MATCH_OFF)
    } else {
        line.replace([MATCH_OPEN, MATCH_CLOSE], "")
    }
}

/// The whole row after its mark dimmed, as one span (the dim tail's own markers dropped: dim
/// spans do not nest).
pub(super) fn dim_after_mark(row: &str, mark: usize) -> String {
    let mut out = String::new();
    let mut seen = 0;
    for c in row.chars().filter(|c| *c != theme::DIM_OPEN && *c != theme::DIM_CLOSE) {
        if !c.is_control() {
            if seen == mark {
                out.push(theme::DIM_OPEN);
            }
            seen += 1;
        }
        out.push(c);
    }
    out.push(theme::DIM_CLOSE);
    out
}

/// The visible character at `at` (control characters are markers and skipped) dimmed: the pin
/// mark, laid out before the markers like every other row piece.
pub(super) fn dim_char_at(row: &str, at: usize) -> String {
    let mut out = String::new();
    let mut seen = 0;
    for c in row.chars() {
        let shown = !c.is_control();
        if shown && seen == at {
            out.push(theme::DIM_OPEN);
            out.push(c);
            out.push(theme::DIM_CLOSE);
        } else {
            out.push(c);
        }
        if shown {
            seen += 1;
        }
    }
    out
}
