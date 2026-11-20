//! One streaming byte scan over a whole transcript: the message count and the title records
//! (`custom-title` from `/rename`) and the `last-prompt` record. They can sit anywhere
//! in a multi-MB file, so the head and tail reads cannot find them. The summary cache keeps the scan's state, so a grown
//! transcript is read only past the last scanned line and an unchanged one not at all.
//! A record is classified from its raw bytes, parsing JSON only for the small title lines.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};

use super::transcript::{clean_text, field, parse_line};
use crate::config::ps_str;

/// What a scan found up to `offset`, and the file it read: the summary cache keeps it, so the
/// next scan of a grown transcript reads only the bytes after `offset`. Titles are cleaned;
/// `None` = no such record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanState {
    /// Bytes consumed: always the end of a complete line, so a line still being written is
    /// read whole next time.
    pub offset: u64,
    /// The file's identity: a resume needs both equal (a replaced file starts over).
    pub created_ms: u64,
    pub file_id: String,
    /// Every non-sidechain user and assistant message; an assistant message Claude Code wrote
    /// as several lines (one per content block, one message id) counts once.
    pub messages: i64,
    /// The last `custom-title` record: a rename beats every earlier one.
    pub custom_title: Option<String>,
    /// The last `ai-title` record (re-emitted every turn; the newest wins).
    pub ai_title: Option<String>,
    /// The last `last-prompt` record: the newest finished turn's prompt, which a long turn's tool
    /// lines can push out of the tail read.
    pub last_prompt: Option<String>,
    /// The message id of the last assistant line, so a message split over the offset counts once.
    pub last_assistant_id: Option<String>,
}

/// One scan's answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scanned {
    /// What the file says now, an unfinished last line included.
    pub found: ScanState,
    /// The state to resume from: complete lines only.
    pub resume: ScanState,
    /// Bytes this scan read.
    pub read_bytes: u64,
}

fn bytes_re(p: &str) -> Regex {
    match Regex::new(p) {
        Ok(r) => r,
        Err(e) => panic!("built-in pattern {p:?} must compile: {e}"),
    }
}

// A structural `"type":"x"` only: inside a JSON string the quotes are escaped, so the literal never
// matches transcript text. The optional suffix is the top-level key that follows the record type
// in Claude Code's key order (user: `message`, assistant: `uuid`); a bare match counts only at the
// start of the line, where the test fixtures and the title records put it.
static MARK: LazyLock<Regex> =
    LazyLock::new(|| bytes_re(r#""type":"(?:custom-title|ai-title|last-prompt|user|assistant)"(?:,"message":\{|,"uuid":")?"#));
static NEWLINE: LazyLock<Regex> = LazyLock::new(|| bytes_re("\n"));
static SIDECHAIN: LazyLock<Regex> = LazyLock::new(|| bytes_re(r#""isSidechain":\s?true"#));
static MESSAGE_ID: LazyLock<Regex> = LazyLock::new(|| bytes_re(r#""message":\{(?:"model":"[^"]*",)?"id":"([^"]+)""#));

/// The top-level keys before a record's type and message id sit in the first few hundred bytes.
const HEAD: usize = 1024;
const CHUNK: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    User,
    Assistant,
    CustomTitle,
    AiTitle,
    LastPrompt,
}

/// The line's record kind, from the first structural type mark that qualifies.
pub(crate) fn kind_of(line: &[u8]) -> Option<Kind> {
    let lead = line.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
    let at_start = line.get(lead) == Some(&b'{') && lead + 1 < line.len();
    // `find_iter`, not captures: the match text alone names the kind, and a capture search costs
    // several times more over a multi-MB file.
    for m in MARK.find_iter(line) {
        let first = at_start && m.start() == lead + 1;
        let text = m.as_bytes().strip_prefix(br#""type":""#.as_slice()).unwrap_or_default();
        let (kind, rest) = [
            (Kind::CustomTitle, b"custom-title\"".as_slice()),
            (Kind::AiTitle, b"ai-title\"".as_slice()),
            (Kind::LastPrompt, b"last-prompt\"".as_slice()),
            (Kind::User, b"user\"".as_slice()),
            (Kind::Assistant, b"assistant\"".as_slice()),
        ]
        .into_iter()
        .find_map(|(k, name)| text.strip_prefix(name).map(|rest| (k, rest)))
        .unwrap_or((Kind::Assistant, b"?".as_slice()));
        let ok = match kind {
            Kind::CustomTitle | Kind::AiTitle | Kind::LastPrompt => first,
            Kind::User => first || rest == br#","message":{"#.as_slice(),
            Kind::Assistant => first || rest == br#","uuid":""#.as_slice(),
        };
        if ok {
            return Some(kind);
        }
    }
    None
}

fn title_of(line: &[u8], key: &str) -> Option<String> {
    let rec = parse_line(&String::from_utf8_lossy(line))?;
    let t = clean_text(&ps_str(field(&rec, key)?));
    (!t.is_empty()).then_some(t)
}

fn classify(line: &[u8], st: &mut ScanState) {
    let Some(kind) = kind_of(line) else { return };
    let head = line.get(..HEAD.min(line.len())).unwrap_or_default();
    match kind {
        Kind::CustomTitle => {
            if let Some(t) = title_of(line, "customTitle") {
                st.custom_title = Some(t);
            }
        }
        Kind::AiTitle => {
            if let Some(t) = title_of(line, "aiTitle") {
                st.ai_title = Some(t);
            }
        }
        Kind::LastPrompt => {
            if let Some(t) = title_of(line, "lastPrompt") {
                st.last_prompt = Some(t);
            }
        }
        Kind::User | Kind::Assistant if SIDECHAIN.is_match(head) => {}
        Kind::User => {
            st.messages += 1;
            st.last_assistant_id = None;
        }
        Kind::Assistant => {
            let id = MESSAGE_ID
                .captures(head)
                .and_then(|c| c.get(1))
                .map(|m| String::from_utf8_lossy(m.as_bytes()).into_owned());
            if id.is_none() || id != st.last_assistant_id {
                st.messages += 1;
            }
            st.last_assistant_id = id;
        }
    }
}

/// Every complete line of `buf`; returns the bytes consumed (through the last newline).
fn scan_lines(buf: &[u8], st: &mut ScanState) -> usize {
    let mut from = 0;
    for nl in NEWLINE.find_iter(buf) {
        classify(buf.get(from..nl.start()).unwrap_or_default(), st);
        from = nl.end();
    }
    from
}

/// The scan over `r`, positioned at `start.offset`: 1 MB chunks, a line longer than a chunk
/// carried until it ends. The bytes after the last newline count in `found` but not in `resume`.
/// A read error ends the scan where it is.
pub(crate) fn scan_reader(mut r: impl Read, start: ScanState) -> Scanned {
    let mut st = start;
    let mut read_bytes = 0u64;
    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK);
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = match r.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => 0,
        };
        if n == 0 {
            break;
        }
        read_bytes += u64::try_from(n).unwrap_or(0);
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
        let used = scan_lines(&buf, &mut st);
        st.offset += u64::try_from(used).unwrap_or(0);
        buf.drain(..used);
    }
    let resume = st.clone();
    if !buf.is_empty() {
        classify(&buf, &mut st);
    }
    Scanned {
        found: st,
        resume,
        read_bytes,
    }
}

fn created_ms(meta: &std::fs::Metadata) -> u64 {
    meta.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// The transcript scanned from `prior`'s offset when `prior` is this same file (identity equal,
/// size not below the offset), else from the start. An unreadable file scans as empty.
pub(crate) fn scan_transcript(path: &Path, prior: Option<&ScanState>) -> Scanned {
    let Ok(mut f) = File::open(path) else {
        return Scanned::default();
    };
    let Ok(meta) = f.metadata() else {
        return Scanned::default();
    };
    let fresh = ScanState {
        created_ms: created_ms(&meta),
        file_id: crate::env::win_fs::file_id(path).unwrap_or_default(),
        ..ScanState::default()
    };
    let start = prior
        .filter(|p| !fresh.file_id.is_empty() && p.file_id == fresh.file_id && p.created_ms == fresh.created_ms && p.offset <= meta.len())
        .cloned()
        .unwrap_or(fresh);
    if start.offset > 0 && f.seek(SeekFrom::Start(start.offset)).is_err() {
        return Scanned::default();
    }
    scan_reader(f, start)
}

#[cfg(test)]
mod tests;
