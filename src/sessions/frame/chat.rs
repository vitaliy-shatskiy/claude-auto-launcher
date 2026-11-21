//! The chat view under the preview card's rule: a chat that fits from the top, a longer one's
//! newest messages filling the pane from the bottom, Claude's on the left, the owner's on the right in the cyan family, each under a dim
//! `who · DD.MM HH:MM` label with its body 2 cells in from that side. Exports `chat_lines`.
//! Markers only; colour comes after layout.

use super::super::chat::ChatMessage;
use super::super::listing::Speaker;
use super::Zone;
use super::markdown::body_lines;
use crate::layout::{display_width, limit_line};
use crate::theme::{DIM_CLOSE, DIM_OPEN, TONE_CLAUDE, TONE_CLOSE, TONE_USER};

/// What the pane has for the highlighted session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatView<'a> {
    Loading,
    Messages(&'a [ChatMessage]),
    /// A cached chat shown while a newer one loads: a dim `updating…` on top.
    Updating(&'a [ChatMessage]),
}

/// A message shows at most this many wrapped lines, then a dim `… N more lines`.
const MAX_LINES: usize = 14;
/// A message block takes at most 3/4 of the pane, never less than this.
const MIN_BLOCK: usize = 16;
/// The widest a message's text wraps, however wide the pane.
const MAX_TEXT: usize = 110;
/// The body's inset from its label's side.
pub(super) const INSET: usize = 2;

/// At most `rows` lines: every message from the top when they fit; else the newest ones ending on
/// the last row, the oldest shown one cut at the top.
pub(super) fn chat_lines(chat: ChatView<'_>, width: usize, rows: usize, ascii: bool, zone: Zone) -> Vec<String> {
    if rows == 0 || width == 0 {
        return Vec::new();
    }
    let dim = |t: &str| format!("{DIM_OPEN}{}{DIM_CLOSE}", limit_line(t, width, ascii));
    let (msgs, rows, banner) = match chat {
        ChatView::Loading => return vec![dim(if ascii { "loading..." } else { "loading\u{2026}" })],
        ChatView::Messages([]) => return vec![dim("(no messages)")],
        ChatView::Messages(m) => (m, rows, None),
        ChatView::Updating(m) => (
            m,
            rows.saturating_sub(1).max(1),
            Some(dim(if ascii { "updating..." } else { "updating\u{2026}" })),
        ),
    };
    // A wide pane keeps readable lines: text wraps at MAX_TEXT, the owner's still at the right edge.
    let block_w = (width * 3 / 4).max(MIN_BLOCK).min(width.saturating_sub(INSET)).min(MAX_TEXT);
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut used = 0;
    for m in msgs.iter().rev() {
        if used >= rows {
            break;
        }
        let base = (m.speaker == Speaker::User).then_some(TONE_USER);
        let body = body_lines(&m.text, block_w, MAX_LINES, ascii, base);
        let block = render(m, &body, width, ascii, zone);
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
    // A chat that fits starts under the rule; only one that overflows ends its newest on the last row.
    let cut = lines.len().saturating_sub(rows);
    let mut out: Vec<String> = banner.into_iter().collect();
    out.extend(lines.into_iter().skip(cut));
    out
}

/// One message: its label line, then its body inset by `INSET`; the owner's block right-aligned
/// with the same inset from the right edge.
fn render(m: &ChatMessage, body: &[String], width: usize, ascii: bool, zone: Zone) -> Vec<String> {
    let dot = if ascii { '-' } else { '\u{B7}' };
    let when = m.at_ms.map(|t| format!(" {dot} {}", zone.stamp(t))).unwrap_or_default();
    let inset = " ".repeat(INSET);
    match m.speaker {
        Speaker::User => {
            let label = format!("you{when}");
            let block = body.iter().map(|l| display_width(l)).max().unwrap_or(0);
            let indent = " ".repeat(width.saturating_sub(block + INSET));
            let mut out = vec![format!(
                "{}{DIM_OPEN}{label}{DIM_CLOSE}",
                " ".repeat(width.saturating_sub(display_width(&label)))
            )];
            out.extend(body.iter().map(|l| format!("{indent}{l}")));
            out
        }
        Speaker::Assistant => {
            let mut out = vec![format!("{TONE_CLAUDE}claude{TONE_CLOSE}{DIM_OPEN}{when}{DIM_CLOSE}")];
            out.extend(body.iter().map(|l| format!("{inset}{l}")));
            out
        }
    }
}
