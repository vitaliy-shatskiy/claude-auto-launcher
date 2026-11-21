//! The picker's chat preview, read side: the newest human and Claude messages of a transcript,
//! streamed backwards from its end in small chunks (`ChatStream`, `read_chat`). Tool calls, tool
//! results, notifications and subagent lines are not messages. `pane` runs it off the UI thread.

pub mod pane;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::listing::Speaker;
use super::scan::{Kind, kind_of};
use super::transcript::{counts_as_prompt, field, parse_line, record_text, user_prompt};
use crate::config::{ps_str, ps_truthy};
use unicode_general_category::{GeneralCategory, get_general_category};

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

/// A transcript read backwards in `CHUNK` steps, newest message first: each `step` reads the
/// chunk before the last, carries the line its start cut to the next step, and classifies every
/// whole line, so a caller can show what it has after any step and stop or resume between steps.
#[derive(Debug)]
pub struct ChatStream<R> {
    r: R,
    pos: u64,
    read: u64,
    cap: u64,
    carry: Vec<u8>,
    newest_first: Vec<(ChatMessage, Option<String>)>,
}

impl ChatStream<File> {
    /// The stream of the transcript at `path`, from its current end; `None` when it cannot be read.
    pub fn open(path: &Path) -> Option<Self> {
        let f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        Some(Self::new(f, len, MAX_READ))
    }
}

impl<R: Read + Seek> ChatStream<R> {
    /// A stream over `r` of `len` bytes that never reads more than `cap`.
    pub fn new(r: R, len: u64, cap: u64) -> Self {
        Self {
            r,
            pos: len,
            read: 0,
            cap,
            carry: Vec::new(),
            newest_first: Vec::new(),
        }
    }

    /// The file's start or the cap was reached: nothing more to read.
    pub fn done(&self) -> bool {
        self.pos == 0 || self.read >= self.cap
    }

    /// Bytes read so far.
    pub fn bytes_read(&self) -> u64 {
        self.read
    }

    /// Messages found so far.
    pub fn found(&self) -> usize {
        self.newest_first.len()
    }

    /// The messages found so far, oldest first.
    pub fn messages(&self) -> Vec<ChatMessage> {
        self.newest_first.iter().rev().map(|(m, _)| m.clone()).collect()
    }

    /// About how many pane rows the messages so far take at `cols` cells: a label and a gap each,
    /// plus their wrapped lines up to the pane's per-message cap.
    pub fn rows(&self, cols: usize) -> usize {
        let cols = cols.max(8);
        self.newest_first
            .iter()
            .map(|(m, _)| {
                let lines: usize = m.text.split('\n').map(|l| crate::layout::display_width(l) / cols + 1).sum();
                2 + lines.min(MESSAGE_ROWS)
            })
            .sum()
    }

    /// Reads one chunk; `false` when done or the read failed (then the stream is done).
    pub fn step(&mut self) -> bool {
        if self.done() {
            return false;
        }
        let take = CHUNK.min(self.pos).min(self.cap - self.read);
        let Some(mut buf) = read_at(&mut self.r, self.pos - take, take) else {
            self.pos = 0;
            return false;
        };
        self.pos -= take;
        self.read += u64::try_from(buf.len()).unwrap_or(take);
        buf.append(&mut self.carry);
        if self.pos > 0 {
            let Some(nl) = buf.iter().position(|b| *b == b'\n') else {
                self.carry = buf;
                return true;
            };
            let rest = buf.split_off(nl + 1);
            buf.pop();
            self.carry = std::mem::replace(&mut buf, rest);
        }
        for line in buf.split(|b| *b == b'\n').rev() {
            take_message(line, &mut self.newest_first);
        }
        true
    }
}

/// One backward read step. Small: the pane shows the first message after one step on a chatty
/// transcript, and a newer cursor target is taken between steps.
pub const CHUNK: u64 = 64 << 10;
/// Never more than this per stream, whatever the transcript's size (they reach 80+ MB of tool
/// output).
pub const MAX_READ: u64 = 8 << 20;
/// The pane shows at most this many lines of one message (`frame::markdown`).
const MESSAGE_ROWS: usize = 15;
/// The pane shows at most ~14 lines of a message; a few KB of text is plenty to wrap.
const MAX_CHARS: usize = 4000;

fn read_at(r: &mut (impl Read + Seek), pos: u64, n: u64) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(pos)).ok()?;
    let mut buf = Vec::with_capacity(usize::try_from(n).unwrap_or(0));
    r.take(n).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// The newest messages of `r` until at least `want` are found, the start, or `cap` bytes.
pub fn read_chat(r: &mut (impl Read + Seek), len: u64, want: usize, cap: u64) -> ChatLoad {
    let mut s = ChatStream::new(r, len, cap);
    while s.found() < want && s.step() {}
    let mut messages = s.messages();
    messages.drain(..messages.len().saturating_sub(want));
    ChatLoad {
        messages,
        bytes_read: s.bytes_read(),
    }
}

/// The start of the text the pane keeps: a message shows at most ~14 wrapped lines, and the
/// markdown pass costs more than linear time on a long reply.
pub(crate) fn bounded(text: &str) -> String {
    text.chars().take(MAX_CHARS).collect()
}

/// Untrusted transcript text for the pane with its line structure kept: control and format
/// characters become spaces, `\r` goes, trailing spaces go, the whole is trimmed.
pub(crate) fn clean_lines(text: &str) -> String {
    let lines: Vec<String> = text.replace('\r', "").split('\n').map(clean_line).collect();
    lines.join("\n").trim_matches('\n').to_owned()
}

fn clean_line(line: &str) -> String {
    let spaced: String = line
        .chars()
        .map(|c| {
            if c == '\t' || matches!(get_general_category(c), GeneralCategory::Control | GeneralCategory::Format) {
                ' '
            } else {
                c
            }
        })
        .collect();
    spaced.trim_end().to_owned()
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
    let text = bounded(&clean_lines(&raw));
    if text.is_empty() {
        return;
    }
    if let Some((newer, newer_id)) = out.last_mut()
        && speaker == Speaker::Assistant
        && newer.speaker == Speaker::Assistant
        && id.is_some()
        && *newer_id == id
    {
        newer.text = bounded(&format!("{text}\n\n{}", newer.text));
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
