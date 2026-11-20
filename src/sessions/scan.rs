//! One streaming byte scan over a whole transcript: the message count and the title records
//! (`custom-title` from `/rename`, `ai-title`). Both can sit anywhere in a multi-MB file, so the
//! head and tail reads cannot find them; the summary cache keeps this to once per file change.
//! A record is classified from its raw bytes, parsing JSON only for the small title lines.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::bytes::Regex;

use super::transcript::{clean_text, field, parse_line};
use crate::config::ps_str;

/// What the scan found. Titles are cleaned; `None` = no such record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TranscriptScan {
    /// Every non-sidechain user and assistant message; an assistant message Claude Code wrote
    /// as several lines (one per content block, one message id) counts once.
    pub messages: i64,
    /// The last `custom-title` record: a rename beats every earlier one.
    pub custom_title: Option<String>,
    /// The last `ai-title` record (re-emitted every turn; the newest wins).
    pub ai_title: Option<String>,
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
    LazyLock::new(|| bytes_re(r#""type":"(?:custom-title|ai-title|user|assistant)"(?:,"message":\{|,"uuid":")?"#));
static NEWLINE: LazyLock<Regex> = LazyLock::new(|| bytes_re("\n"));
static SIDECHAIN: LazyLock<Regex> = LazyLock::new(|| bytes_re(r#""isSidechain":\s?true"#));
static MESSAGE_ID: LazyLock<Regex> = LazyLock::new(|| bytes_re(r#""message":\{(?:"model":"[^"]*",)?"id":"([^"]+)""#));

/// The top-level keys before a record's type and message id sit in the first few hundred bytes.
const HEAD: usize = 1024;
const CHUNK: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    User,
    Assistant,
    CustomTitle,
    AiTitle,
}

#[derive(Debug, Default)]
struct State {
    scan: TranscriptScan,
    last_assistant_id: Option<Vec<u8>>,
}

/// The line's record kind, from the first structural type mark that qualifies.
fn kind_of(line: &[u8]) -> Option<Kind> {
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
            (Kind::User, b"user\"".as_slice()),
            (Kind::Assistant, b"assistant\"".as_slice()),
        ]
        .into_iter()
        .find_map(|(k, name)| text.strip_prefix(name).map(|rest| (k, rest)))
        .unwrap_or((Kind::Assistant, b"?".as_slice()));
        let ok = match kind {
            Kind::CustomTitle | Kind::AiTitle => first,
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

fn classify(line: &[u8], st: &mut State) {
    let Some(kind) = kind_of(line) else { return };
    let head = line.get(..HEAD.min(line.len())).unwrap_or_default();
    match kind {
        Kind::CustomTitle => {
            if let Some(t) = title_of(line, "customTitle") {
                st.scan.custom_title = Some(t);
            }
        }
        Kind::AiTitle => {
            if let Some(t) = title_of(line, "aiTitle") {
                st.scan.ai_title = Some(t);
            }
        }
        Kind::User | Kind::Assistant if SIDECHAIN.is_match(head) => {}
        Kind::User => {
            st.scan.messages += 1;
            st.last_assistant_id = None;
        }
        Kind::Assistant => {
            let id = MESSAGE_ID.captures(head).and_then(|c| c.get(1)).map(|m| m.as_bytes().to_vec());
            if id.is_none() || id != st.last_assistant_id {
                st.scan.messages += 1;
            }
            st.last_assistant_id = id;
        }
    }
}

/// Every complete line of `buf` (the last one only when `last`); returns the bytes consumed.
fn scan_lines(buf: &[u8], last: bool, st: &mut State) -> usize {
    let mut from = 0;
    for nl in NEWLINE.find_iter(buf) {
        classify(buf.get(from..nl.start()).unwrap_or_default(), st);
        from = nl.end();
    }
    if last && from < buf.len() {
        classify(buf.get(from..).unwrap_or_default(), st);
        return buf.len();
    }
    from
}

/// The whole file in 1 MB chunks; a line longer than a chunk is carried until it ends. An
/// unreadable file scans as empty.
pub(crate) fn scan_transcript(path: &Path) -> TranscriptScan {
    let Ok(mut f) = File::open(path) else {
        return TranscriptScan::default();
    };
    let mut st = State::default();
    let mut buf: Vec<u8> = Vec::with_capacity(CHUNK);
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = match f.read(&mut chunk) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => 0,
        };
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
        let used = scan_lines(&buf, n == 0, &mut st);
        buf.drain(..used);
        if n == 0 {
            break;
        }
    }
    st.scan
}

#[cfg(test)]
mod tests;
