//! The chat view under the preview card's rule: the newest messages filling the pane from the
//! bottom, Claude's on the left, the owner's on the right in the cyan family, each with a dim
//! `who · DD.MM HH:MM` label. Exports `chat_lines`. Markers only; colour comes after layout.

use super::super::chat::ChatMessage;
use super::super::listing::Speaker;
use super::text::local_stamp;
use crate::layout::{display_width, ellipsis, limit_cells_right, limit_line, split_text_lines};
use crate::theme::{DIM_CLOSE, DIM_OPEN, TONE_CLAUDE, TONE_CLOSE, TONE_USER, strip_ctl};

/// What the pane has for the highlighted session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatView<'a> {
    Loading,
    Messages(&'a [ChatMessage]),
}

/// An older message shows at most this many lines (the last one ends in `…`).
const OLDER_LINES: usize = 8;
/// A message block takes at most 3/4 of the pane, never less than this.
const MIN_BLOCK: usize = 16;

/// `rows` lines: the newest messages, bottom-aligned, the oldest shown one cut at the top. The
/// newest message always ends on the last row; when it alone is taller than the pane its END shows.
pub(super) fn chat_lines(chat: ChatView<'_>, width: usize, rows: usize, ascii: bool, offset: i32) -> Vec<String> {
    if rows == 0 || width == 0 {
        return Vec::new();
    }
    let dim = |t: &str| format!("{DIM_OPEN}{}{DIM_CLOSE}", limit_line(t, width, ascii));
    let msgs = match chat {
        ChatView::Loading => return vec![dim(if ascii { "loading..." } else { "loading\u{2026}" })],
        ChatView::Messages([]) => return vec![dim("(no messages)")],
        ChatView::Messages(m) => m,
    };
    let block_w = (width * 3 / 4).max(MIN_BLOCK).min(width);
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut used = 0;
    for (i, m) in msgs.iter().enumerate().rev() {
        if used >= rows {
            break;
        }
        let text = strip_ctl(&m.text);
        let body = if i + 1 == msgs.len() {
            tail(&text, block_w, rows.saturating_sub(1).max(1), ascii)
        } else {
            split_text_lines(&text, block_w, OLDER_LINES, ascii)
        };
        let block = render(m, &body, width, ascii, offset);
        used += block.len() + 1;
        blocks.push(block);
    }
    let mut lines: Vec<String> = Vec::new();
    for b in blocks.into_iter().rev() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(b);
    }
    let cut = lines.len().saturating_sub(rows);
    let mut out = vec![String::new(); rows.saturating_sub(lines.len())];
    out.extend(lines.into_iter().skip(cut));
    out
}

/// The text wrapped, keeping its last `room` lines; a cut start is marked with `…`.
fn tail(text: &str, width: usize, room: usize, ascii: bool) -> Vec<String> {
    let mut all = split_text_lines(text, width, 0, ascii);
    if all.len() <= room {
        return all;
    }
    let mut kept = all.split_off(all.len() - room);
    if let Some(first) = kept.first_mut() {
        *first = format!("{}{}", ellipsis(ascii), limit_cells_right(first, width.saturating_sub(1)));
    }
    kept
}

/// One message: its label line, then its text; the owner's block right-aligned.
fn render(m: &ChatMessage, body: &[String], width: usize, ascii: bool, offset: i32) -> Vec<String> {
    let dot = if ascii { '-' } else { '\u{B7}' };
    let when = m.at_ms.map(|t| format!(" {dot} {}", local_stamp(t, offset))).unwrap_or_default();
    match m.speaker {
        Speaker::User => {
            let label = format!("you{when}");
            let block = body.iter().map(|l| display_width(l)).max().unwrap_or(0);
            let indent = " ".repeat(width.saturating_sub(block));
            let mut out = vec![format!(
                "{}{DIM_OPEN}{label}{DIM_CLOSE}",
                " ".repeat(width.saturating_sub(display_width(&label)))
            )];
            out.extend(body.iter().map(|l| format!("{indent}{TONE_USER}{l}{TONE_CLOSE}")));
            out
        }
        Speaker::Assistant => {
            let mut out = vec![format!("{TONE_CLAUDE}claude{TONE_CLOSE}{DIM_OPEN}{when}{DIM_CLOSE}")];
            out.extend(body.iter().cloned());
            out
        }
    }
}
