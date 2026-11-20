//! A chat message's markdown laid out for the pane: its line structure kept (paragraphs, list
//! items with a hanging indent, headings bold, code fences and tables dimmed), inline emphasis and
//! code spans styled instead of printed, every line word-wrapped to the width. Exports
//! `body_lines`. Markers only; colour comes after layout.

use std::sync::LazyLock;

use regex::Regex;

use crate::layout::char_width;
use crate::theme::{DIM_CLOSE, DIM_OPEN, TONE_BOLD, TONE_CLOSE, TONE_CODE, re, strip_ctl};

/// How a run of text shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Plain,
    Bold,
    Code,
    Dim,
}

/// A list item: its indent, its marker (`-`, `*`, `+` or a number), its text.
static ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^(\s*)([-*+]|\d{1,3}[.)])\s+(.*)$"));
static HEADING: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*#{1,6}\s+(.*?)\s*#*\s*$"));
static FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(```|~~~)"));
static TABLE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*\|"));
static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`]+)`"));
static LINK: LazyLock<Regex> = LazyLock::new(|| re(r"!?\[([^\]]+)\]\([^)\s]*\)"));
/// The pairing rules of `plain`: a non-word neighbour outside and no space inside, `/`, `\` and
/// `.` counted as word characters, so paths, globs and `__init__` stay. Bold first, then single.
static EMPHASIS: LazyLock<[(Regex, bool); 4]> = LazyLock::new(|| {
    [
        (re(r"(^|[^\w*/\\.])\*\*([^\s*]|[^\s*].*?\S)\*\*($|[^\w*/\\.])"), true),
        (re(r"(^|[^\w_/\\.])__([^\s_]|[^\s_].*?\S)__($|[^\w_/\\.])"), true),
        (re(r"(^|[^\w*/\\.])\*([^\s*]|[^\s*][^*]*?\S)\*($|[^\w*/\\.])"), false),
        (re(r"(^|[^\w_/\\.])_([^\s_]|[^\s_][^_]*?\S)_($|[^\w_/\\.])"), false),
    ]
});
/// In-text bold marks while the emphasis passes run; never in cleaned text (`strip_ctl`).
const B_OPEN: char = '\u{1c}';
const B_CLOSE: char = '\u{1d}';

/// The message laid out at `width` cells: each source line wrapped on its own, blank lines folded
/// to one, the whole capped at `max` lines plus a dim `… N more lines`. `base` wraps plain runs
/// (the owner's tone), `None` leaves them bare.
pub(super) fn body_lines(text: &str, width: usize, max: usize, ascii: bool, base: Option<char>) -> Vec<String> {
    let width = width.max(4);
    let mut out: Vec<Vec<(String, Look)>> = Vec::new();
    let mut in_fence = false;
    let mut blank = false;
    for raw in text.split('\n') {
        let line = strip_ctl(raw);
        if FENCE.is_match(&line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || TABLE.is_match(&line) {
            if blank {
                out.push(Vec::new());
                blank = false;
            }
            out.extend(cut(line.trim_end(), width));
            continue;
        }
        if line.trim().is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push(Vec::new());
            blank = false;
        }
        if let Some(c) = HEADING.captures(&line) {
            let runs: Vec<(String, Look)> = inline(c.get(1).map_or("", |m| m.as_str()))
                .into_iter()
                .map(|(t, l)| (t, if l == Look::Code { l } else { Look::Bold }))
                .collect();
            out.extend(wrap(&runs, "", 0, width));
        } else if let Some(c) = ITEM.captures(&line) {
            let lead = c.get(1).map_or(0, |m| m.as_str().len()).min(6);
            let mark = c.get(2).map_or("", |m| m.as_str());
            let bullet = if mark.starts_with(|ch: char| ch.is_ascii_digit()) {
                mark.to_owned()
            } else if ascii {
                "-".to_owned()
            } else {
                "\u{2022}".to_owned()
            };
            let prefix = format!("{}{bullet} ", " ".repeat(lead));
            let hang = prefix.chars().map(char_width).sum();
            out.extend(wrap(&inline(c.get(3).map_or("", |m| m.as_str())), &prefix, hang, width));
        } else {
            out.extend(wrap(&inline(line.trim_end()), "", 0, width));
        }
    }
    while out.last().is_some_and(Vec::is_empty) {
        out.pop();
    }
    let more = out.len().saturating_sub(max);
    if more > 0 {
        out.truncate(max);
        let dots = if ascii { "..." } else { "\u{2026}" };
        let s = if more == 1 { "" } else { "s" };
        out.push(vec![(format!("{dots} {more} more line{s}"), Look::Dim)]);
    }
    out.iter().map(|l| paint(l, base)).collect()
}

/// One line's inline markdown as runs: links to their text, code spans verbatim, paired emphasis
/// markers dropped (bold kept as `Bold`).
fn inline(text: &str) -> Vec<(String, Look)> {
    let text = LINK.replace_all(text, "$1");
    let mut runs = Vec::new();
    let mut at = 0;
    for c in CODE.captures_iter(&text) {
        let (Some(whole), Some(inner)) = (c.get(0), c.get(1)) else {
            continue;
        };
        emphasis(text.get(at..whole.start()).unwrap_or(""), &mut runs);
        runs.push((inner.as_str().to_owned(), Look::Code));
        at = whole.end();
    }
    emphasis(text.get(at..).unwrap_or(""), &mut runs);
    runs
}

fn emphasis(text: &str, runs: &mut Vec<(String, Look)>) {
    let mut s = text.to_owned();
    for (r, bold) in EMPHASIS.iter() {
        let with = if *bold {
            format!("$1{B_OPEN}$2{B_CLOSE}$3")
        } else {
            "$1$2$3".to_owned()
        };
        // A match consumes its outer neighbour, so `**a** **b**` takes a second pass.
        loop {
            let next = r.replace_all(&s, with.as_str()).into_owned();
            if next == s {
                break;
            }
            s = next;
        }
    }
    let mut look = Look::Plain;
    let mut cur = String::new();
    for ch in s.chars() {
        let next = match ch {
            B_OPEN => Look::Bold,
            B_CLOSE => Look::Plain,
            _ => {
                cur.push(ch);
                continue;
            }
        };
        if !cur.is_empty() {
            runs.push((std::mem::take(&mut cur), look));
        }
        look = next;
    }
    if !cur.is_empty() {
        runs.push((cur, look));
    }
}

/// Greedy word wrap of styled runs: the first line opens with `prefix`, the rest hang `hang`
/// cells in; a word wider than the line is cut by cells.
fn wrap(runs: &[(String, Look)], prefix: &str, hang: usize, width: usize) -> Vec<Vec<(String, Look)>> {
    // Words as styled chars; a run boundary inside a word keeps both looks.
    let mut words: Vec<Vec<(char, Look)>> = Vec::new();
    let mut word: Vec<(char, Look)> = Vec::new();
    for (text, look) in runs {
        for ch in text.chars() {
            if ch == ' ' {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            } else {
                word.push((ch, *look));
            }
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let start = || -> (Vec<(char, Look)>, usize) { (Vec::new(), 0) };
    let mut lines: Vec<(Vec<(char, Look)>, usize)> = Vec::new();
    let mut indent = prefix.chars().map(char_width).sum::<usize>();
    let (mut cur, mut used) = start();
    for w in words {
        let ww: usize = w.iter().map(|(c, _)| char_width(*c)).sum();
        let room = width.saturating_sub(indent).max(1);
        let gap = usize::from(!cur.is_empty());
        if used + gap + ww <= room {
            if gap == 1 {
                cur.push((' ', Look::Plain));
            }
            cur.extend(w);
            used += gap + ww;
            continue;
        }
        if !cur.is_empty() {
            lines.push((std::mem::take(&mut cur), indent));
            indent = hang;
            used = 0;
        }
        let room = width.saturating_sub(indent).max(1);
        for (c, l) in w {
            let cw = char_width(c);
            if used + cw > room && used > 0 {
                lines.push((std::mem::take(&mut cur), indent));
                indent = hang;
                used = 0;
            }
            cur.push((c, l));
            used += cw;
        }
    }
    lines.push((cur, indent));
    lines
        .into_iter()
        .enumerate()
        .map(|(i, (chars, indent))| {
            let lead = if i == 0 { prefix.to_owned() } else { " ".repeat(indent) };
            let mut runs: Vec<(String, Look)> = vec![(lead, Look::Plain)];
            for (c, l) in chars {
                match runs.last_mut() {
                    Some((t, last)) if *last == l => t.push(c),
                    _ => runs.push((c.to_string(), l)),
                }
            }
            runs.retain(|(t, _)| !t.is_empty());
            runs
        })
        .collect()
}

/// A raw line (code, a table row) cut by cells, its spacing kept, dimmed.
fn cut(line: &str, width: usize) -> Vec<Vec<(String, Look)>> {
    let mut out = vec![String::new()];
    let mut used = 0;
    for c in line.chars() {
        let cw = char_width(c);
        if used + cw > width && used > 0 {
            out.push(String::new());
            used = 0;
        }
        if let Some(l) = out.last_mut() {
            l.push(c);
        }
        used += cw;
    }
    out.into_iter().map(|l| vec![(l, Look::Dim)]).collect()
}

/// Runs to one line of markers.
fn paint(runs: &[(String, Look)], base: Option<char>) -> String {
    let mut out = String::new();
    for (t, look) in runs {
        let (open, close) = match look {
            Look::Plain => match base {
                Some(b) => (b.to_string(), TONE_CLOSE.to_string()),
                None => (String::new(), String::new()),
            },
            Look::Bold => (TONE_BOLD.to_string(), TONE_CLOSE.to_string()),
            Look::Code => (TONE_CODE.to_string(), TONE_CLOSE.to_string()),
            Look::Dim => (DIM_OPEN.to_string(), DIM_CLOSE.to_string()),
        };
        out.push_str(&open);
        out.push_str(t);
        out.push_str(&close);
    }
    out
}

#[cfg(test)]
mod tests;
