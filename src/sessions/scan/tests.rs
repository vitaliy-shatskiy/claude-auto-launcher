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
    assert_eq!(scan_transcript(&fixture("counted.jsonl")).messages, 7);
    assert_eq!(scan_transcript(&fixture("with-sidechain.jsonl")).messages, 2);
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
    assert_eq!(scan_transcript(&p).messages, 6, "2 prompts + 1 tool result + m1 + m2 + m2 again");
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
    let s = scan_transcript(&p);
    assert_eq!(s.custom_title.as_deref(), Some("across the boundary"));
    assert_eq!(s.messages, 1, "the last line has no newline and still counts");
}

#[test]
fn hostile_title_text_is_cleaned_and_an_empty_one_ignored() {
    let text = [custom("good"), custom("\\u001b[31mred\\u0007 bell"), custom("   ")].join("\n");
    let (_t, p) = transcript("hostile", &text);
    assert_eq!(scan_transcript(&p).custom_title.as_deref(), Some("[31mred bell"));
}
