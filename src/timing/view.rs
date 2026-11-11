//! What debug mode shows: the F12 timings screen, and the footer line under every screen when
//! `CLAUDE_AUTO_DEBUG=footer` asks for it.

use super::{Span, Timings, ms};
use crate::frame::frame_width;
use crate::input::{Event, vk};
use crate::layout::{display_width, limit_line};
use crate::theme;
use crate::ui::{Term, UiStop};

/// The footer text for the widest tier that fits `width`: full words, then short words, then
/// bare numbers cut to the frame. A value not measured yet reads `-`.
pub fn footer_text(load: Option<u64>, paint: Option<u64>, key: Option<u64>, width: usize, ascii: bool) -> String {
    let v = |m: Option<u64>| m.map_or_else(|| "-".to_owned(), |m| m.to_string());
    let (l, p, k) = (v(load), v(paint), v(key));
    let sep = if ascii { " | " } else { " · " };
    let fw = frame_width(width);
    [
        format!("  load {l} ms{sep}paint {p} ms{sep}last key->frame {k} ms"),
        format!("  load {l}{sep}paint {p}{sep}key {k} ms"),
    ]
    .into_iter()
    .find(|t| display_width(t) <= fw)
    .unwrap_or_else(|| limit_line(&format!("{l}/{p}/{k} ms"), fw, ascii))
}

/// Footer mode: the screen draws one row shorter, the footer owns the last row.
pub fn body_size(t: &Timings, size: (usize, usize)) -> (usize, usize) {
    if t.shows_footer() {
        (size.0, size.1.saturating_sub(1))
    } else {
        size
    }
}

/// Footer mode: `lines` cut or padded to the body height, then the dim footer line. Otherwise
/// untouched, debug on or off.
pub fn with_footer(t: &Timings, mut lines: Vec<String>, screen: &str, size: (usize, usize)) -> Vec<String> {
    if !t.shows_footer() {
        return lines;
    }
    let body = size.1.saturating_sub(1);
    lines.truncate(body);
    lines.resize(body, String::new());
    let (color, ascii) = t.style();
    let text = footer_text(
        t.latest_ms(&format!("load:{screen}")),
        t.latest_ms(&format!("paint:{screen}")),
        t.latest_ms(&format!("key:{screen}")),
        size.0,
        ascii,
    );
    lines.push(if color {
        format!("{}{text}{}", theme::DIM, theme::RESET)
    } else {
        text
    });
    lines
}

/// The spans as an indented tree, slowest first within a level; open spans count up to `now_us`.
pub fn tree_lines(spans: &[Span], now_us: u64) -> Vec<String> {
    let mut out = Vec::new();
    walk(spans, None, 0, now_us, &mut out);
    out
}

fn walk(spans: &[Span], parent: Option<usize>, depth: usize, now_us: u64, out: &mut Vec<String>) {
    let dur = |s: &Span| s.dur_us.unwrap_or_else(|| now_us.saturating_sub(s.start_us));
    let mut kids: Vec<(usize, &Span)> = spans
        .iter()
        .enumerate()
        .filter(|(_, s)| s.parent == parent && !s.discarded)
        .collect();
    kids.sort_by_key(|(_, s)| std::cmp::Reverse(dur(s)));
    for (i, s) in kids {
        let open = if s.dur_us.is_none() {
            " (open)"
        } else if s.aborted {
            " (aborted)"
        } else {
            ""
        };
        out.push(format!("{}{}  {:.2} ms{open}", "  ".repeat(depth), s.name, ms(dur(s))));
        walk(spans, Some(i), depth + 1, now_us, out);
    }
}

/// The F12 screen at one size: title, the tree window from `top`, the key hint.
pub fn timings_frame(tree: &[String], top: usize, size: (usize, usize), ascii: bool) -> Vec<String> {
    let fw = frame_width(size.0);
    let visible = size.1.saturating_sub(3);
    let mut lines = vec![limit_line("  timings of this run, slowest first", fw, ascii), String::new()];
    lines.extend(
        tree.iter()
            .skip(top)
            .take(visible)
            .map(|l| limit_line(&format!("  {l}"), fw, ascii)),
    );
    lines.resize(size.1.saturating_sub(1), String::new());
    lines.push(limit_line("  esc back   up/down scroll", fw, ascii));
    lines.truncate(size.1.max(1));
    lines
}

/// The F12 screen: Escape, Enter or F12 returns to the screen it was opened from.
pub fn show_timings(term: &mut dyn Term, t: &Timings) -> Result<(), UiStop> {
    let tree = tree_lines(&t.spans(), t.now_us());
    let (_, ascii) = t.style();
    let mut top = 0usize;
    loop {
        let size = term.size();
        term.paint(&timings_frame(&tree, top, size, ascii));
        let page = size.1.saturating_sub(3).max(1);
        let ev = loop {
            match term.wait(size)? {
                Event::Idle => {}
                e => break e,
            }
        };
        match ev {
            Event::Key(k) if k.is(vk::ESCAPE) || k.is(vk::ENTER) || k.is(vk::F12) || k.is_ctrl(vk::C) => return Ok(()),
            Event::Key(k) if k.is(vk::UP) => top = top.saturating_sub(1),
            Event::Key(k) if k.is(vk::DOWN) => top = (top + 1).min(tree.len().saturating_sub(1)),
            Event::Key(k) if k.is(vk::PAGE_UP) => top = top.saturating_sub(page),
            Event::Key(k) if k.is(vk::PAGE_DOWN) => top = (top + page).min(tree.len().saturating_sub(1)),
            Event::Key(_) | Event::Mouse(_) | Event::Resize | Event::Idle => {}
        }
    }
}

#[cfg(test)]
mod tests;
