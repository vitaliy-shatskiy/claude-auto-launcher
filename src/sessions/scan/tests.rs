use super::*;
use crate::sessions::listing::summarize;
use crate::sessions::transcript::MAX_BYTES;
use std::fmt::Write as _;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

struct Tmp(PathBuf);

impl Drop for Tmp {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// A transcript under a slug directory of its own temp root.
fn transcript(tag: &str, text: &str) -> (Tmp, PathBuf) {
    let root = std::env::temp_dir().join(format!("cars-scan-{tag}-{}", std::process::id()));
    let dir = root.join("C--src-demo");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("s1.jsonl");
    std::fs::write(&p, text).unwrap();
    (Tmp(root), p)
}

const USER: &str = r#"{"parentUuid":null,"isSidechain":false,"promptId":"p1","type":"user","message":{"role":"user","content":"first question"},"uuid":"u1"}"#;

fn assistant(id: &str, block: &str) -> String {
    format!(
        r#"{{"parentUuid":"u1","isSidechain":false,"message":{{"model":"claude-x","id":"{id}","type":"message","role":"assistant","content":[{block}]}},"requestId":"r","type":"assistant","uuid":"a-{id}"}}"#
    )
}

const TOOL_RESULT: &str = r#"{"parentUuid":"a","isSidechain":false,"promptId":"p1","type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]},"uuid":"u2"}"#;
/// Nested records an attachment carries are not messages of this transcript.
const ATTACHMENT: &str = r#"{"parentUuid":"a","isSidechain":false,"attachment":{"items":[{"type":"user","text":"x"},{"type":"assistant","text":"y"}]},"type":"attachment","uuid":"t1"}"#;
const SIDE: &str =
    r#"{"parentUuid":"a","isSidechain":true,"message":{"model":"m","id":"side","content":[]},"type":"assistant","uuid":"s1"}"#;

#[test]
fn counts_every_user_and_assistant_message_of_the_fixture() {
    // counted.jsonl: 5 user records (prompts, a tool result, a meta) and 2 assistant replies;
    // the sidechain prompt is not this session's.
    assert_eq!(scan_transcript(&fixture("counted.jsonl"), None).found.messages, 7);
    assert_eq!(scan_transcript(&fixture("with-sidechain.jsonl"), None).found.messages, 2);
}

#[test]
fn an_assistant_message_split_over_lines_counts_once_and_nested_types_never() {
    let lines = [
        USER.to_owned(),
        assistant("m1", r#"{"type":"thinking","thinking":"\"type\":\"user\""}"#),
        assistant("m1", r#"{"type":"tool_use","name":"Read","input":{}}"#),
        TOOL_RESULT.to_owned(),
        ATTACHMENT.to_owned(),
        SIDE.to_owned(),
        assistant("m2", r#"{"type":"text","text":"done"}"#),
        USER.to_owned(),
        assistant("m2", r#"{"type":"text","text":"a resent id after a user is a new message"}"#),
    ];
    let (_t, p) = transcript("split", &lines.join("\n"));
    assert_eq!(
        scan_transcript(&p, None).found.messages,
        6,
        "2 prompts + 1 tool result + m1 + m2 + m2 again"
    );
}

/// A long transcript: `n` prompt lines of ~1 KB, `extra` lines spliced in at their indexes.
fn long_file(n: usize, extra: &[(usize, String)]) -> String {
    let pad = "x".repeat(1000);
    let mut out = String::new();
    for i in 0..n {
        for (at, l) in extra {
            if *at == i {
                out.push_str(l);
                out.push('\n');
            }
        }
        writeln!(out, r#"{{"type":"user","message":{{"role":"user","content":"prompt {i} {pad}"}}}}"#).unwrap();
    }
    out
}

fn custom(t: &str) -> String {
    format!(r#"{{"type":"custom-title","customTitle":"{t}","sessionId":"s1"}}"#)
}

fn ai(t: &str) -> String {
    format!(r#"{{"type":"ai-title","aiTitle":"{t}","sessionId":"s1"}}"#)
}

fn last_prompt(t: &str) -> String {
    format!(r#"{{"type":"last-prompt","lastPrompt":"{t}","leafUuid":"l1","sessionId":"s1"}}"#)
}

fn prompt(t: &str) -> String {
    format!(r#"{{"parentUuid":null,"isSidechain":false,"type":"user","message":{{"role":"user","content":"{t}"}},"uuid":"u-{t}"}}"#)
}

/// A first prompt, the last real prompt, its `last-prompt` record, then 300 tool lines (past the
/// 120-line tail) and the reply's text.
fn long_turn(extra_after_tools: &str) -> String {
    let mut lines = vec![prompt("first ask"), prompt("final ask"), last_prompt("final ask")];
    for i in 0..300 {
        lines.push(assistant(&format!("t{i}"), r#"{"type":"tool_use","name":"Read","input":{}}"#));
        lines.push(TOOL_RESULT.to_owned());
    }
    lines.push(assistant("end", r#"{"type":"text","text":"the answer"}"#));
    if !extra_after_tools.is_empty() {
        lines.push(extra_after_tools.to_owned());
    }
    lines.join("\n") + "\n"
}

#[test]
fn a_last_prompt_outside_the_tail_window_is_the_last_prompt() {
    let (_t, p) = transcript("lastp", &long_turn(""));
    let s = summarize(&p, None, true, MAX_BYTES).unwrap();
    assert_eq!(s.first_prompt, "first ask");
    assert_eq!(s.last_user, "final ask", "not the first prompt the card fell back to");
    assert_eq!(s.last_assistant, "the answer", "the reply to that prompt");
}

#[test]
fn a_prompt_in_the_tail_newer_than_the_last_prompt_record_wins() {
    // Claude Code writes `last-prompt` after a turn: a prompt still being answered has none yet.
    let (_t, p) = transcript("lastp-newer", &long_turn(&prompt("newest ask")));
    assert_eq!(summarize(&p, None, true, MAX_BYTES).unwrap().last_user, "newest ask");
}

#[test]
fn a_resumed_scan_picks_up_a_newer_last_prompt() {
    let head = [USER.to_owned(), last_prompt("older ask")].join("\n") + "\n";
    let (_t, p) = transcript("lastp-grow", &head);
    let first = scan_transcript(&p, None);
    assert_eq!(first.found.last_prompt.as_deref(), Some("older ask"));
    let tail = [
        USER.to_owned(),
        last_prompt("newer ask"),
        assistant("m9", r#"{"type":"text","text":"x"}"#),
    ]
    .join("\n")
        + "\n";
    append(&p, &tail);
    let again = scan_transcript(&p, Some(&first.resume));
    assert_eq!(again.read_bytes, len(&tail), "only the appended bytes");
    assert_eq!(again.found.last_prompt.as_deref(), Some("newer ask"));
    assert_eq!(again.found, scan_transcript(&p, None).found, "a full scan's answer");
}

#[test]
fn a_custom_title_in_the_middle_of_a_long_file_beats_the_ai_title() {
    // 2,000 lines of ~1 KB: past the 400-line head and the 120-line tail the summary reads.
    let text = long_file(2000, &[(10, ai("old ai")), (1000, custom("claude-auto")), (1900, ai("new ai"))]);
    let (_t, p) = transcript("custom", &text);
    let s = summarize(&p, None, true, MAX_BYTES).unwrap();
    assert_eq!(s.title, "claude-auto");
    assert_eq!(s.first_prompt.split(' ').take(2).collect::<Vec<_>>(), ["prompt", "0"]);
    assert_eq!(s.message_count, 2000);
}

#[test]
fn the_last_ai_title_wins_without_a_rename() {
    let text = long_file(300, &[(5, ai("first guess")), (250, ai("better guess"))]);
    let (_t, p) = transcript("ai", &text);
    assert_eq!(summarize(&p, None, true, MAX_BYTES).unwrap().title, "better guess");
}

#[test]
fn the_first_prompt_titles_a_session_without_title_records() {
    let (_t, p) = transcript("plain", &long_file(3, &[]));
    let s = summarize(&p, None, true, MAX_BYTES).unwrap();
    assert!(s.title.starts_with("prompt 0 xxx"), "{}", s.title);
    assert_eq!(s.title, s.first_prompt);
}

#[test]
fn a_title_line_straddling_a_chunk_boundary_is_read_whole() {
    let line = custom("across the boundary");
    let filler = format!(r#"{{"type":"x","pad":"{}"}}"#, "y".repeat(CHUNK - 20 - 18));
    let text = format!("{filler}\n{line}\n{USER}");
    assert!(text.find(&line).unwrap() < CHUNK && text.find(&line).unwrap() + line.len() > CHUNK);
    let (_t, p) = transcript("straddle", &text);
    let s = scan_transcript(&p, None).found;
    assert_eq!(s.custom_title.as_deref(), Some("across the boundary"));
    assert_eq!(s.messages, 1, "the last line has no newline and still counts");
}

#[test]
fn hostile_title_text_is_cleaned_and_an_empty_one_ignored() {
    let text = [custom("good"), custom("\\u001b[31mred\\u0007 bell"), custom("   ")].join("\n");
    let (_t, p) = transcript("hostile", &text);
    assert_eq!(scan_transcript(&p, None).found.custom_title.as_deref(), Some("[31mred bell"));
}

/// Counts what passes through: the bytes a scan really read.
struct Counting<R> {
    inner: R,
    read: u64,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += u64::try_from(n).unwrap();
        Ok(n)
    }
}

fn append(p: &Path, text: &str) {
    use std::io::Write as _;
    std::fs::OpenOptions::new()
        .append(true)
        .open(p)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

/// The fields a resumed scan must agree on with a full one.
fn answer(s: &ScanState) -> (i64, Option<String>, Option<String>) {
    (s.messages, s.custom_title.clone(), s.ai_title.clone())
}

fn len(s: &str) -> u64 {
    u64::try_from(s.len()).unwrap()
}

#[test]
fn a_grown_transcript_is_read_only_past_the_last_scanned_line() {
    let head = [USER.to_owned(), assistant("m1", r#"{"type":"thinking"}"#), custom("first name")].join("\n") + "\n";
    let (_t, p) = transcript("grow", &head);
    let first = scan_transcript(&p, None);
    assert_eq!(first.read_bytes, len(&head));
    assert_eq!(first.resume.offset, first.read_bytes);
    // The same message continues past the offset: it still counts once.
    let tail = [assistant("m1", r#"{"type":"text","text":"x"}"#), ai("later ai"), USER.to_owned()].join("\n") + "\n";
    append(&p, &tail);
    let again = scan_transcript(&p, Some(&first.resume));
    assert_eq!(again.read_bytes, len(&tail), "only the appended bytes");
    assert_eq!(
        answer(&again.found),
        answer(&scan_transcript(&p, None).found),
        "a full scan's answer"
    );
    assert_eq!(answer(&again.found), (3, Some("first name".into()), Some("later ai".into())));
    let still = scan_transcript(&p, Some(&again.resume));
    assert_eq!(still.read_bytes, 0, "an unchanged file reads nothing");
}

#[test]
fn the_injected_reader_sees_only_the_bytes_after_the_offset() {
    let text = [USER.to_owned(), USER.to_owned(), custom("t")].join("\n") + "\n";
    let start = ScanState {
        offset: len(USER) + 1,
        messages: 1,
        ..ScanState::default()
    };
    let rest = text.as_bytes().get(USER.len() + 1..).unwrap();
    let mut r = Counting { inner: rest, read: 0 };
    let s = scan_reader(&mut r, start);
    assert_eq!(r.read, u64::try_from(rest.len()).unwrap());
    assert_eq!((s.found.messages, s.resume.offset), (2, len(&text)));
}

#[test]
fn a_shrunk_or_replaced_file_is_scanned_from_the_start() {
    let long = [USER.to_owned(), USER.to_owned(), USER.to_owned()].join("\n") + "\n";
    let (_t, p) = transcript("shrink", &long);
    let before = scan_transcript(&p, None).resume;
    std::fs::write(&p, format!("{USER}\n")).unwrap();
    let shrunk = scan_transcript(&p, Some(&before));
    assert_eq!((shrunk.found.messages, shrunk.read_bytes), (1, len(USER) + 1));

    let before = scan_transcript(&p, None).resume;
    std::fs::remove_file(&p).unwrap();
    let replaced = format!("{long}{}\n", custom("new file"));
    std::fs::write(&p, &replaced).unwrap();
    let s = scan_transcript(&p, Some(&before));
    assert_ne!(s.resume.file_id, before.file_id, "a new file under the same name");
    assert_eq!(s.read_bytes, len(&replaced), "read from the start");
    assert_eq!(answer(&s.found), (3, Some("new file".into()), None));
}

#[test]
fn a_line_cut_by_the_old_end_of_file_is_read_whole_next_time() {
    let line = custom("half and half");
    let (cut, rest) = line.split_at(line.len() / 2);
    let (_t, p) = transcript("cut", &format!("{USER}\n{cut}"));
    let first = scan_transcript(&p, None);
    assert_eq!(first.resume.offset, len(USER) + 1, "the offset stops at the last newline");
    assert_eq!(first.found.custom_title, None, "half a record is no title");
    append(&p, &format!("{rest}\n{USER}\n"));
    let again = scan_transcript(&p, Some(&first.resume));
    assert_eq!(answer(&again.found), (2, Some("half and half".into()), None));
    assert_eq!(again.read_bytes, len(&line) + 1 + len(USER) + 1, "the cut line again, whole");
}
