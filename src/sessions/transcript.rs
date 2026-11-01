//! Transcript reading (`Sessions.ps1` first half): which record is a human prompt, the clean text a
//! frame may show, the slug fallback, bounded head/tail reads, the prompt counter, the headless
//! verdict and the `claude -c` candidate test. Pure apart from reading the file it is handed.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use unicode_general_category::{GeneralCategory, get_general_category};

use crate::config::{ps_str, ps_truthy};
use crate::theme::re;

/// Wrapper text Claude Code injects around real prompts.
const NOISE_PREFIXES: [&str; 7] = [
    "<local-command-caveat>",
    "<command-name>",
    "<command-message>",
    "<command-args>",
    "<local-command-stdout>",
    "<system-reminder>",
    "<task-notification>",
];

/// The 4 MB byte budget of every bounded walk.
pub(crate) const MAX_BYTES: usize = 4 * 1024 * 1024;

static NOISE_TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^<[a-z][a-z0-9-]*(-notification|-hook|-reminder|-caveat|-stdout)>"));
static COMMAND_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"<command-name>([^<]*)</command-name>"));
static COMMAND_ARGS: LazyLock<Regex> = LazyLock::new(|| re(r"<command-args>([^<]*)</command-args>"));
// `-match` is case-insensitive in PowerShell, so every line pattern below carries (?i).
static USER_TYPE: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"type"\s*:\s*"user""#));
static CHAT_TYPE: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"type"\s*:\s*"(user|assistant)""#));
static TOOL_RESULT: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"type"\s*:\s*"tool_result""#));
static SIDECHAIN: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"isSidechain"\s*:\s*true"#));
static META: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"isMeta"\s*:\s*true"#));
static MESSAGE_OPEN: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"message"\s*:\s*\{"#));
static CONTENT_PLAIN: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)^"content"\s*:\s*"[^<"\\\s]"#));
static FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)```[^\n]*\r?\n(.*?)```"));
static ENTRYPOINT: LazyLock<Regex> = LazyLock::new(|| re(r#""entrypoint":\s?"([^"]*)""#));
static ENTRYPOINT_OPEN: LazyLock<Regex> = LazyLock::new(|| re(r#""entrypoint":\s?""#));
static SIDECHAIN_HEAD: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"isSidechain":\s?true"#));
static TEAM: LazyLock<Regex> = LazyLock::new(|| re(r#""teamName":\s?"([^"]*)""#));
static KIND: LazyLock<Regex> = LazyLock::new(|| re(r#""sessionKind":\s?"([^"]*)""#));

/// A JSON property as PowerShell reads it: exact name first, then case-insensitively.
pub(crate) fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    let o = v.as_object()?;
    o.get(name)
        .or_else(|| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))
}

fn type_is(v: &Value, name: &str) -> bool {
    field(v, "type").is_some_and(|t| ps_str(t).eq_ignore_ascii_case(name))
}

/// The summary's head pre-filter: a line that may be a user record.
pub(crate) fn is_user_line(line: &str) -> bool {
    USER_TYPE.is_match(line)
}

/// The tail pre-filter: a line that may be a user or an assistant record.
pub(crate) fn is_chat_line(line: &str) -> bool {
    CHAT_TYPE.is_match(line)
}

/// `ConvertFrom-JsonlLine`: one record, or `None` for a blank or malformed line.
pub(crate) fn parse_line(line: &str) -> Option<Value> {
    if line.trim().is_empty() {
        return None;
    }
    serde_json::from_str(line).ok()
}

fn blocks(content: &Value) -> Vec<&Value> {
    match content {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![content],
        _ => Vec::new(),
    }
}

/// `Get-ClaudeRecordText`: a plain string, or the `text` blocks of an array joined by a space.
pub(crate) fn record_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    if let Value::String(s) = content {
        return Some(s.clone());
    }
    let text: Vec<String> = blocks(content)
        .into_iter()
        .filter(|b| type_is(b, "text"))
        .filter_map(|b| field(b, "text").filter(|t| !t.is_null()).map(ps_str))
        .collect();
    (!text.is_empty()).then(|| text.join(" "))
}

fn content_of(rec: &Value) -> Option<&Value> {
    field(rec, "message").and_then(|m| field(m, "content"))
}

/// `Get-ClaudeUserPrompt`: the text a human typed, or `None` when the record is machinery.
pub(crate) fn user_prompt(rec: &Value) -> Option<String> {
    if !type_is(rec, "user") || ps_truthy(field(rec, "isSidechain")) {
        return None;
    }
    let content = content_of(rec);
    if let Some(c) = content
        && !c.is_string()
        && blocks(c).iter().any(|b| type_is(b, "tool_result"))
    {
        return None;
    }
    let text = record_text(content)?;
    if text.trim().is_empty() {
        return None;
    }
    let trimmed = text.trim_start();
    if trimmed.starts_with("<command-message>") || trimmed.starts_with("<command-name>") {
        let group = |r: &Regex| {
            r.captures(trimmed)
                .and_then(|c| c.get(1))
                .map_or("", |m| m.as_str())
                .trim()
                .to_owned()
        };
        let args = group(&COMMAND_ARGS);
        return (!args.is_empty()).then(|| format!("{} {args}", group(&COMMAND_NAME)).trim().to_owned());
    }
    if ps_truthy(field(rec, "isMeta")) || NOISE_PREFIXES.iter().any(|p| trimmed.starts_with(p)) || NOISE_TAG.is_match(trimmed) {
        return None;
    }
    Some(text)
}

/// `Get-CodeFenceCollapsedText`: a fenced block becomes `[code: N lines]`.
pub(crate) fn collapse_fences(text: &str) -> String {
    if !text.contains("```") {
        return text.to_owned();
    }
    FENCE
        .replace_all(text, |c: &regex::Captures<'_>| {
            let body = c.get(1).map_or("", |m| m.as_str()).trim_end_matches(['\r', '\n']);
            let n = if body.is_empty() { 0 } else { body.split('\n').count() };
            format!("[code: {n} lines]")
        })
        .into_owned()
}

/// `Get-CleanTranscriptText`: control and format characters become spaces, whitespace runs one
/// space, trimmed. Transcript text is untrusted and reaches the terminal.
pub fn clean_text(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| {
            if matches!(get_general_category(c), GeneralCategory::Control | GeneralCategory::Format) {
                ' '
            } else {
                c
            }
        })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `ConvertFrom-ClaudeProjectSlug`: (project, worktree) guessed from a slug, the fallback when no
/// transcript records a cwd.
pub(crate) fn slug_names(slug: &str) -> (String, String) {
    let marker = "--claude-worktrees-";
    let (main, worktree) = match slug.find(marker) {
        Some(i) => (slug.get(..i).unwrap_or(slug), slug.get(i + marker.len()..).unwrap_or("")),
        None => (slug, ""),
    };
    let known = "Projects-";
    let project = match main.find(known) {
        Some(j) => main.get(j + known.len()..).unwrap_or("").to_owned(),
        None => main.split('-').rfind(|s| !s.is_empty()).unwrap_or("").to_owned(),
    };
    (project, worktree.to_owned())
}

fn decode(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes).into_owned();
    match s.strip_prefix('\u{FEFF}') {
        Some(t) => t.to_owned(),
        None => s,
    }
}

/// The first `count` lines of a file (`Get-Content -TotalCount`); nothing when unreadable.
pub(crate) fn head_lines(path: &Path, count: usize) -> Vec<String> {
    let Ok(f) = File::open(path) else { return Vec::new() };
    let mut r = BufReader::new(f);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    while out.len() < count {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                while matches!(buf.last(), Some(b'\n' | b'\r')) {
                    buf.pop();
                }
                out.push(decode(&buf));
            }
        }
    }
    out
}

fn read_at(f: &mut File, offset: u64, count: usize) -> Vec<u8> {
    let mut buf = vec![0u8; count];
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return Vec::new();
    }
    let mut got = 0;
    while got < count {
        match f.read(buf.get_mut(got..).unwrap_or_default()) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    buf.truncate(got);
    buf
}

/// `Get-FileTailLines`: the last `count` lines, walking back in chunks and bounded by BYTES as well
/// as newlines, so a newline-free file costs the budget and never the whole file.
pub(crate) fn tail_lines(path: &Path, count: usize, max_bytes: usize) -> Vec<String> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let Ok(len) = f.metadata().map(|m| m.len()) else {
        return Vec::new();
    };
    if len == 0 {
        return Vec::new();
    }
    let chunk = 65_536u64;
    let max = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    let mut pos = len;
    let mut walked = 0u64;
    let mut newlines = 0usize;
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    while pos > 0 && newlines <= count && walked < max {
        let take = chunk.min(pos).min(max - walked);
        pos -= take;
        walked += take;
        let buf = read_at(&mut f, pos, usize::try_from(take).unwrap_or(0));
        if buf.is_empty() {
            break;
        }
        newlines += count_newlines(&buf);
        chunks.push(buf);
    }
    let all: Vec<u8> = chunks.into_iter().rev().flatten().collect();
    let text = String::from_utf8_lossy(&all);
    let mut lines: Vec<String> = text.split('\n').map(|l| l.trim_end_matches('\r').to_owned()).collect();
    if pos > 0 && lines.len() > 1 {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.len() > count {
        lines.drain(..lines.len() - count);
    }
    lines
}

#[expect(clippy::naive_bytecount, reason = "one count per 64 KB chunk; no dependency for it")]
fn count_newlines(buf: &[u8]) -> usize {
    buf.iter().filter(|b| **b == b'\n').count()
}

/// The first `max_bytes` of a file as lines, plus the rest of the record straddling the boundary.
fn bounded_lines(path: &Path, max_bytes: usize) -> Option<Vec<String>> {
    let mut f = File::open(path).ok()?;
    let mut buf = read_at(&mut f, 0, max_bytes);
    if buf.last().is_some_and(|b| *b != b'\n') {
        let mut rest = BufReader::new(f);
        let mut tail = Vec::new();
        if rest.read_until(b'\n', &mut tail).is_ok() {
            if tail.last() == Some(&b'\n') {
                tail.pop();
            }
            buf.extend(tail);
        }
    }
    Some(split_lines(&decode(&buf)))
}

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned()).collect()
}

/// A match of `re` whose opening quote is structural (not preceded by a backslash).
fn structural(re: &Regex, line: &str) -> bool {
    re.find_iter(line)
        .any(|m| m.start() == 0 || line.as_bytes().get(m.start() - 1) != Some(&b'\\'))
}

/// The MESSAGE's content as a plain string opening with an ordinary character: a human prompt
/// that needs no parse. Hand-coded for the two lookbehinds the regex crate lacks.
fn plain_prompt_content(line: &str) -> bool {
    for m in MESSAGE_OPEN.find_iter(line) {
        if m.start() > 0 && line.as_bytes().get(m.start() - 1) == Some(&b'\\') {
            continue;
        }
        let rest = line.get(m.end()..).unwrap_or("");
        let mut prev = '{';
        for (n, (i, c)) in rest.char_indices().enumerate() {
            if n > 400 {
                break;
            }
            if prev != '\\' && CONTENT_PLAIN.is_match(rest.get(i..).unwrap_or("")) {
                return true;
            }
            if c == '{' || c == '}' {
                break;
            }
            prev = c;
        }
    }
    false
}

fn counts_as_prompt(line: &str) -> bool {
    if !line.contains("\"user\"") {
        return false;
    }
    if (line.contains("\"tool_result\"") && structural(&TOOL_RESULT, line))
        || (line.contains("\"isSidechain\"") && structural(&SIDECHAIN, line))
        || (line.contains("\"isMeta\"") && structural(&META, line))
        || (line.contains("\"type\"") && !structural(&USER_TYPE, line))
    {
        return false;
    }
    plain_prompt_content(line) || parse_line(line).is_some_and(|r| user_prompt(&r).is_some())
}

/// `Measure-ClaudePromptDetail`: (count, capped). Past `max_bytes` the count stops and says so.
pub(crate) fn prompt_detail(path: &Path, max_bytes: usize) -> (i64, bool) {
    let bounded = std::fs::metadata(path).map_or(true, |m| max_bytes > 0 && m.len() > u64::try_from(max_bytes).unwrap_or(u64::MAX));
    let lines = if bounded {
        bounded_lines(path, max_bytes).unwrap_or_default()
    } else {
        match std::fs::read(path) {
            Ok(b) => split_lines(&decode(&b)),
            Err(_) => return (0, false),
        }
    };
    let n = lines.iter().filter(|l| counts_as_prompt(l)).count();
    (i64::try_from(n).unwrap_or(i64::MAX), bounded)
}

/// `Test-HeadlessEntrypoint`.
pub(crate) fn headless_entrypoint(e: &str) -> bool {
    e.starts_with("sdk-")
}

fn has_non_headless(seen: &str) -> bool {
    ENTRYPOINT_OPEN
        .find_iter(seen)
        .any(|m| !seen.get(m.end()..).unwrap_or("").starts_with("sdk-"))
}

/// `Get-TranscriptHeadlessVerdict`: `Some(true)` when every entrypoint seen is headless,
/// `Some(false)` when one is not, `None` when none is seen or the file cannot be read.
pub(crate) fn headless_verdict(path: &Path, window: usize) -> Option<bool> {
    let mut f = File::open(path).ok()?;
    let len = usize::try_from(f.metadata().ok()?.len()).unwrap_or(usize::MAX);
    let head_max = if len <= window.saturating_mul(2) { len } else { window };
    let mut head = Vec::new();
    while head.len() < head_max {
        let piece = read_at(&mut f, u64::try_from(head.len()).unwrap_or(0), 8192.min(head_max - head.len()));
        if piece.is_empty() {
            break;
        }
        head.extend(piece);
        if head_max < len && ENTRYPOINT.is_match(&String::from_utf8_lossy(&head)) {
            break;
        }
    }
    let mut seen = String::from_utf8_lossy(&head).into_owned();
    if let Some(c) = ENTRYPOINT.captures(&seen)
        && !headless_entrypoint(c.get(1).map_or("", |m| m.as_str()))
    {
        return Some(false);
    }
    if head_max < len {
        let tail = read_at(&mut f, u64::try_from(len - window).unwrap_or(0), window);
        seen.push('\n');
        seen.push_str(&String::from_utf8_lossy(&tail));
    }
    if !ENTRYPOINT.is_match(&seen) {
        return None;
    }
    Some(!has_non_headless(&seen))
}

/// `Test-HeadlessTranscript`: no entrypoint or an unreadable file is NOT headless.
pub(crate) fn is_headless(path: &Path) -> bool {
    headless_verdict(path, 65_536) == Some(true)
}

/// `Test-ContinueCandidate`: whether `claude -c` considers this transcript at all.
pub(crate) fn continue_candidate(path: &Path, window: usize) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    let Ok(len) = f.metadata().map(|m| m.len()) else { return false };
    let w = u64::try_from(window).unwrap_or(u64::MAX);
    let head = String::from_utf8_lossy(&read_at(&mut f, 0, usize::try_from(w.min(len)).unwrap_or(0))).into_owned();
    let tail = if len > w {
        String::from_utf8_lossy(&read_at(&mut f, len - w, window)).into_owned()
    } else {
        head.clone()
    };
    if SIDECHAIN_HEAD.is_match(&head) {
        return false;
    }
    let group = |r: &Regex, t: &str| r.captures(t).and_then(|c| c.get(1)).map(|m| m.as_str().to_owned());
    if group(&TEAM, &head).is_some_and(|t| !t.is_empty()) {
        return false;
    }
    let kind_text = head.split('\n').find(|l| l.contains("\"parentUuid\":")).unwrap_or(&head);
    if group(&KIND, kind_text).is_some_and(|k| k.eq_ignore_ascii_case("daemon") || k.eq_ignore_ascii_case("daemon-worker")) {
        return false;
    }
    let entry = group(&ENTRYPOINT, &head).unwrap_or_else(|| {
        ENTRYPOINT
            .captures_iter(&tail)
            .last()
            .and_then(|c| c.get(1))
            .map_or_else(String::new, |m| m.as_str().to_owned())
    });
    !["sdk-cli", "sdk-ts", "sdk-py"].iter().any(|e| e.eq_ignore_ascii_case(&entry))
}

#[cfg(test)]
mod tests;
