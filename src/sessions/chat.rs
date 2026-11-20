//! The picker's chat preview, read side: the newest human and Claude messages of a transcript,
//! read backwards from its end in bounded steps (`read_chat`, `load_chat`). Tool calls, tool
//! results, notifications and subagent lines are not messages. `pane` runs it off the UI thread.

pub mod pane;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::frame::plain::plain;
use super::listing::Speaker;
use super::scan::{Kind, kind_of};
use super::transcript::{clean_text, collapse_fences, counts_as_prompt, field, parse_line, record_text, user_prompt};
use crate::config::{ps_str, ps_truthy};

/// One message as the chat pane shows it: who, the plain text, when (UTC ms) if recorded.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatMessage {
    pub speaker: Speaker,
    pub text: String,
    pub at_ms: Option<i64>,
}

/// A load's answer: the messages oldest first, and the bytes it read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatLoad {
    pub messages: Vec<ChatMessage>,
    pub bytes_read: u64,
}

/// The first read from the end; each further step reads as much again, further back.
pub const FIRST_READ: u64 = 1 << 20;
/// Never more than this per load, whatever the transcript's size (they reach 80+ MB).
pub const MAX_READ: u64 = 8 << 20;
/// The pane shows at most ~8 lines of an older message; a few KB of text is plenty to wrap.
const MAX_CHARS: usize = 4000;

/// The newest `want` messages of the transcript at `path`; empty when it cannot be read.
pub fn load_chat(path: &Path, want: usize) -> ChatLoad {
    let Ok(mut f) = File::open(path) else { return ChatLoad::default() };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return ChatLoad::default();
    };
    read_chat(&mut f, len, want, MAX_READ)
}

fn read_at(r: &mut (impl Read + Seek), pos: u64, n: u64) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
    r.take(n).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Walks back from `len` in steps (1 MB, then 1 MB more each time) until `want` messages are
/// found, the file starts, or `cap` bytes were read. A line cut by a step's start is carried to the
/// next step, so every line is classified whole.
pub fn read_chat(r: &mut (impl Read + Seek), len: u64, want: usize, cap: u64) -> ChatLoad {
    let mut pos = len;
    let mut read = 0u64;
    let mut carry: Vec<u8> = Vec::new();
    let mut newest_first: Vec<(ChatMessage, Option<String>)> = Vec::new();
    while pos > 0 && read < cap && newest_first.len() < want {
        let take = FIRST_READ.min(pos).min(cap - read);
        let Some(mut buf) = read_at(r, pos - take, take) else { break };
        pos -= take;
        read += u64::try_from(buf.len()).unwrap_or(take);
        buf.append(&mut carry);
        if pos > 0 {
            let Some(nl) = buf.iter().position(|b| *b == b'\n') else {
                carry = buf;
                continue;
            };
            let rest = buf.split_off(nl + 1);
            buf.pop();
            carry = std::mem::replace(&mut buf, rest);
        }
        for line in buf.split(|b| *b == b'\n').rev() {
            if newest_first.len() >= want {
                break;
            }
            take_message(line, &mut newest_first);
        }
    }
    let messages = newest_first.into_iter().rev().map(|(m, _)| m).collect();
    ChatLoad {
        messages,
        bytes_read: read,
    }
}

/// Which end of a long message the pane keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keep {
    /// An older message: shown from its start.
    Head,
    /// The newest message: shown from its end.
    Tail,
}

impl Keep {
    /// `Tail` for the newest message, `Head` for any other.
    pub(crate) fn of(newest: bool) -> Self {
        if newest { Self::Tail } else { Self::Head }
    }
}

/// The text kept for the pane: `keep`'s end of it. Cut BEFORE the markdown pass: `plain` costs
/// more than linear time on a long reply.
pub(crate) fn bounded(text: &str, keep: Keep) -> String {
    let count = text.chars().count();
    if count <= MAX_CHARS {
        return text.to_owned();
    }
    match keep {
        Keep::Tail => text.chars().skip(count - MAX_CHARS).collect(),
        Keep::Head => text.chars().take(MAX_CHARS).collect(),
    }
}

/// Classifies one line, newest first; an assistant message written as several lines (one per
/// content block, one message id) joins the text of the line after it.
fn take_message(line: &[u8], out: &mut Vec<(ChatMessage, Option<String>)>) {
    let Some(kind) = kind_of(line) else { return };
    let text = String::from_utf8_lossy(line);
    let (speaker, raw, id, rec) = match kind {
        Kind::User => {
            if !counts_as_prompt(&text) {
                return;
            }
            let Some(rec) = parse_line(&text) else { return };
            let Some(t) = user_prompt(&rec) else { return };
            (Speaker::User, t, None, rec)
        }
        Kind::Assistant => {
            let Some(rec) = parse_line(&text) else { return };
            if ps_truthy(field(&rec, "isSidechain")) {
                return;
            }
            let msg = field(&rec, "message");
            let Some(t) = record_text(msg.and_then(|m| field(m, "content"))) else {
                return;
            };
            let id = msg.and_then(|m| field(m, "id")).map(ps_str);
            (Speaker::Assistant, t, id, rec)
        }
        Kind::CustomTitle | Kind::AiTitle | Kind::LastPrompt => return,
    };
    let text = plain(&bounded(&clean_text(&collapse_fences(&raw)), Keep::of(out.is_empty())));
    if text.is_empty() {
        return;
    }
    let newest = out.len() == 1;
    if let Some((newer, newer_id)) = out.last_mut()
        && speaker == Speaker::Assistant
        && newer.speaker == Speaker::Assistant
        && id.is_some()
        && *newer_id == id
    {
        newer.text = bounded(&format!("{text} {}", newer.text), Keep::of(newest));
        return;
    }
    let at_ms = field(&rec, "timestamp").and_then(|t| iso_ms(&ps_str(t)));
    out.push((ChatMessage { speaker, text, at_ms }, id));
}

/// `2026-11-20T16:06:12.345Z` (UTC) as ms since the epoch; `None` for any other shape.
fn iso_ms(s: &str) -> Option<i64> {
    let num = |a: usize, b: usize| s.get(a..b).and_then(|t| t.parse::<i64>().ok());
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if s.get(10..11) != Some("T") || !(1..=12).contains(&mo) {
        return None;
    }
    let days = crate::sys::days_from_civil(i32::try_from(y).ok()?, u32::try_from(mo).ok()?, u32::try_from(d).ok()?);
    Some(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000)
}

#[cfg(test)]
mod tests;
