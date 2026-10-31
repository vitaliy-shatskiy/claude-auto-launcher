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
mod tests {
    use super::*;

    fn rec(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn slug_parse_worktree_projects_and_single_segment() {
        assert_eq!(
            slug_names("C--Users-someone-Desktop-Projects-Demo-Service--claude-worktrees-feature-1"),
            ("Demo-Service".into(), "feature-1".into())
        );
        assert_eq!(slug_names("C--Users-someone-Desktop-Projects-Demo"), ("Demo".into(), String::new()));
        assert_eq!(slug_names("subagents").0, "subagents", "a single segment is the whole segment");
        assert_eq!(slug_names("C--Users-someone-Desktop-Scratch").0, "Scratch");
        assert_eq!(
            slug_names("C--Users-someone-Desktop-Pro\u{AD}jects-Demo").0,
            "Demo",
            "Projects- is matched ordinally"
        );
        assert_eq!(
            slug_names("C--Users-someone-Desktop-Projects-Demo-\u{AD}-claude-worktrees-feature").1,
            ""
        );
    }

    #[test]
    fn fences_collapse_with_real_line_counts() {
        let plain = "plain text with `inline` backticks, no fence at all";
        assert_eq!(collapse_fences(plain), plain);
        let fenced = "Here:\n```powershell\nWrite-Host 'a'\nWrite-Host 'b'\nWrite-Host 'c'\n```\nDone.";
        assert_eq!(collapse_fences(fenced), "Here:\n[code: 3 lines]\nDone.");
        assert_eq!(
            collapse_fences("```\none\n``` and ```\nx\ny\n```"),
            "[code: 1 lines] and [code: 2 lines]"
        );
    }

    #[test]
    fn clean_text_turns_markers_and_escapes_into_spaces() {
        assert_eq!(clean_text("a\u{4}b\u{5}c\u{1}d\u{2}e"), "a b c d e");
        assert_eq!(clean_text("x\u{1F}y"), "x y");
        assert_eq!(clean_text("\u{1b}]0;pwned\u{7}hello \u{1b}[2Jworld"), "]0;pwned hello [2Jworld");
    }

    #[test]
    fn user_prompt_rules() {
        assert_eq!(
            user_prompt(&rec(r#"{"type":"user","message":{"content":"hi"}}"#)).as_deref(),
            Some("hi")
        );
        assert_eq!(
            user_prompt(&rec(r#"{"type":"user","isSidechain":true,"message":{"content":"x"}}"#)),
            None
        );
        assert_eq!(
            user_prompt(&rec(
                r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#
            )),
            None
        );
        let cmd = r#"{"type":"user","message":{"content":"<command-name>/doctor</command-name><command-args>fix it</command-args>"}}"#;
        assert_eq!(user_prompt(&rec(cmd)).as_deref(), Some("/doctor fix it"));
        let bare = r#"{"type":"user","message":{"content":"<command-name>/clear</command-name><command-args></command-args>"}}"#;
        assert_eq!(user_prompt(&rec(bare)), None);
        assert_eq!(
            user_prompt(&rec(r#"{"type":"user","message":{"content":"<bash-stdout>x</bash-stdout>"}}"#)),
            None
        );
        assert_eq!(
            user_prompt(&rec(r#"{"type":"user","message":{"content":"<task-notification>x"}}"#)),
            None
        );
    }

    fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cars-w1-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tail_walk_is_bounded_by_bytes() {
        let d = temp("tail");
        let p = write(&d, "t.txt", &format!("{}\ntail line\n", "x".repeat(1 << 20)));
        let lines = tail_lines(&p, 5, 65_536);
        assert_eq!(lines.last().map(String::as_str), Some("tail line"));
        let longest = lines.iter().map(String::len).max().unwrap_or(0);
        assert!(longest > 0 && longest <= 70_000, "longest {longest}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
    }

    #[test]
    fn prompt_counts_agree_with_the_fixtures() {
        assert_eq!(prompt_detail(&fixture("counted.jsonl"), MAX_BYTES).0, 3);
        assert_eq!(prompt_detail(&fixture("bare-clear.jsonl"), MAX_BYTES).0, 0);
        assert_eq!(prompt_detail(&fixture("slash-command.jsonl"), MAX_BYTES).0, 1);
        assert_eq!(prompt_detail(&fixture("task-notification.jsonl"), MAX_BYTES).0, 1);
        assert_eq!(prompt_detail(&fixture("embedded-tag.jsonl"), MAX_BYTES).0, 1);
    }

    #[test]
    fn cap_boundary_both_ends_and_bytes_not_chars() {
        let d = temp("bound");
        let line = |i: usize| format!(r#"{{"type":"user","message":{{"content":"prompt-{i:02}"}}}}"#);
        let lines = |n: usize| (1..=n).map(line).collect::<Vec<_>>().join("\n") + "\n";
        let exact = write(&d, "exact.jsonl", &lines(4));
        assert_eq!(std::fs::metadata(&exact).unwrap().len(), 200);
        assert_eq!(prompt_detail(&exact, 200), (4, false), "exactly the budget is read whole");
        let over = write(&d, "over.jsonl", &lines(5));
        assert_eq!(prompt_detail(&over, 200), (4, true), "one byte over engages the bound");
        let straddle = format!(
            "{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"{}\"}}]}}}}\n{{\"type\":\"user\",\"message\":{{\"content\":\"{}\"}}}}\n",
            "p".repeat(84),
            "q".repeat(60)
        );
        assert_eq!(prompt_detail(&write(&d, "s.jsonl", &straddle), 200), (1, true));
        let cyr = (1..=40)
            .map(|_| format!(r#"{{"type":"user","message":{{"content":"{}"}}}}"#, "\u{43F}".repeat(50)))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert_eq!(prompt_detail(&write(&d, "c.jsonl", &cyr), 1000).0, 8, "the budget is BYTES");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn count_agrees_with_its_authority() {
        let d = temp("auth");
        let cases = [
            r#"{"type":"user","message":{"role":"user","content":"fix the build"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent chatter"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"<system-reminder>noise</system-reminder>"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<local-command-stdout>build output</local-command-stdout>"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<command-name>/clear</command-name><command-args></command-args>"}]}}"#,
            r#"{"type": "user", "message": {"role": "user", "content": "a real prompt"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"   "}}"#,
            r#"{"type":"user","toolUseResult":{"content":"tool output here"},"message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}"#,
            r#"{"type":"user","toolUseResult":{"content":"tool output here"},"message":{"role":"user","content":"fix the build"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"why is \"tool_result\" in the docs"}}"#,
        ];
        for (i, c) in cases.iter().enumerate() {
            let authority = i64::from(parse_line(c).and_then(|r| user_prompt(&r)).is_some());
            let p = write(&d, &format!("case{i}.jsonl"), &format!("{c}\n"));
            assert_eq!(prompt_detail(&p, MAX_BYTES).0, authority, "case {i}: {c}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn headless_and_continue_candidates() {
        let d = temp("cont");
        let rec = |e: &str| format!(r#"{{"type":"user","entrypoint":"{e}","message":{{"content":"x"}}}}"#);
        assert!(is_headless(&write(&d, "a.jsonl", &rec("sdk-cli"))));
        assert!(!is_headless(&write(&d, "b.jsonl", &rec("cli"))));
        assert!(
            !is_headless(&write(&d, "c.jsonl", r#"{"type":"user"}"#)),
            "no entrypoint is interactive"
        );
        let pad = format!(r#"{{"type":"progress","data":"{}"}}"#, "p".repeat(1000));
        let sdktail = format!(
            "{{\"type\":\"user\",\"message\":{{\"content\":\"x\"}}}}\n{}{{\"type\":\"last-prompt\",\"entrypoint\":\"sdk-py\"}}",
            format!("{pad}\n").repeat(80)
        );
        let rejected = [
            rec("sdk-cli"),
            sdktail,
            r#"{"type":"user","isSidechain":true,"message":{"content":"x"}}"#.to_owned(),
            r#"{"type":"user","teamName":"alpha","message":{"content":"x"}}"#.to_owned(),
            "{\"type\":\"file-history-snapshot\"}\n{\"parentUuid\":null,\"type\":\"user\",\"sessionKind\":\"daemon\",\"message\":{\"content\":\"x\"}}".to_owned(),
        ];
        for (i, text) in rejected.iter().enumerate() {
            assert!(!continue_candidate(&write(&d, &format!("r{i}.jsonl"), text), 65_536), "case {i}");
        }
        assert!(continue_candidate(&write(&d, "ok.jsonl", &rec("cli")), 65_536));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
