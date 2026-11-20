use super::*;
use std::io::Cursor;

fn fixture() -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/chat.jsonl", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn texts(load: &ChatLoad) -> Vec<(Speaker, &str)> {
    load.messages.iter().map(|m| (m.speaker, m.text.as_str())).collect()
}

#[test]
fn only_human_prompts_and_claude_text_are_messages_their_markdown_kept() {
    // Given: a transcript with tool calls and results, reminders, notifications, a subagent, a
    // meta record, a system record, thinking, and one reply split over three lines.
    let bytes = fixture();
    let len = u64::try_from(bytes.len()).unwrap();
    // When: the chat is read.
    let load = read_chat(&mut Cursor::new(bytes), len, 50, MAX_READ);
    // Then: four messages, oldest first, markdown and line breaks kept for the renderer, the
    // split reply joined in order a paragraph apart.
    assert_eq!(
        texts(&load),
        [
            (Speaker::User, "first question about the **parser**"),
            (Speaker::Assistant, "## Plan\n- read `scan.rs` first\n\nthen *fix* it"),
            (Speaker::User, "second question"),
            (Speaker::Assistant, "**Done:** shipped"),
        ]
    );
    assert_eq!(load.messages[2].at_ms, iso_ms("2026-11-20T18:06:00.000Z"));
    assert_eq!(load.bytes_read, len);
}

#[test]
fn want_stops_the_walk_at_the_newest_messages() {
    let bytes = fixture();
    let len = u64::try_from(bytes.len()).unwrap();
    let load = read_chat(&mut Cursor::new(bytes), len, 2, MAX_READ);
    assert_eq!(
        texts(&load),
        [(Speaker::User, "second question"), (Speaker::Assistant, "**Done:** shipped")]
    );
}

#[test]
fn a_long_message_keeps_its_head_the_newest_too() {
    let older = format!("HEAD {} TAILOLD", "word ".repeat(2000));
    let newest = format!("START {} END", "more ".repeat(2000));
    let line = |t: &str, text: &str, id: &str| {
        format!(r#"{{"type":"{t}","message":{{"id":"{id}","role":"{t}","content":"{text}"}},"uuid":"{id}"}}"#)
    };
    let text = format!("{}\n{}\n", line("user", &older, "u"), line("user", &newest, "v"));
    let len = u64::try_from(text.len()).unwrap();
    let load = read_chat(&mut Cursor::new(text.into_bytes()), len, 10, MAX_READ);
    assert!(load.messages[0].text.starts_with("HEAD") && !load.messages[0].text.ends_with("TAILOLD"));
    assert!(load.messages[1].text.starts_with("START") && !load.messages[1].text.ends_with("END"));
}

#[test]
fn a_line_cut_by_a_step_boundary_is_read_whole() {
    // Given: a 1.5 MB tool result line before the newest exchange, so the first 1 MB step ends inside it.
    let mut text = String::from(r#"{"type":"user","message":{"role":"user","content":"early prompt"},"uuid":"e"}"#);
    text.push('\n');
    text.push_str(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":""#);
    text.push_str(&"x".repeat(1_500_000));
    text.push_str(r#""}]},"uuid":"t"}"#);
    text.push('\n');
    text.push_str(r#"{"type":"user","message":{"role":"user","content":"late prompt"},"uuid":"l"}"#);
    text.push('\n');
    let len = u64::try_from(text.len()).unwrap();
    let load = read_chat(&mut Cursor::new(text.into_bytes()), len, 10, MAX_READ);
    assert_eq!(texts(&load), [(Speaker::User, "early prompt"), (Speaker::User, "late prompt")]);
}

/// An 83 MB transcript of tool output only, generated on the fly, counting what is read.
struct Huge {
    len: u64,
    pos: u64,
    read: u64,
}

impl Read for Huge {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        const LINE: &[u8] = br#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"log"}]}}
"#;
        let n = usize::try_from(self.len.saturating_sub(self.pos)).unwrap().min(buf.len());
        for (i, b) in buf.iter_mut().take(n).enumerate() {
            *b = LINE[(usize::try_from(self.pos).unwrap() + i) % LINE.len()];
        }
        self.pos += u64::try_from(n).unwrap();
        self.read += u64::try_from(n).unwrap();
        Ok(n)
    }
}

impl Seek for Huge {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        if let SeekFrom::Start(p) = to {
            self.pos = p;
        }
        Ok(self.pos)
    }
}

#[test]
fn a_huge_transcript_without_messages_costs_the_cap_and_no_more() {
    let mut r = Huge {
        len: 83 * 1024 * 1024,
        pos: 0,
        read: 0,
    };
    let len = r.len;
    let load = read_chat(&mut r, len, 30, MAX_READ);
    assert_eq!(load.messages, []);
    assert_eq!(r.read, MAX_READ, "bytes the reader handed out");
    assert_eq!(load.bytes_read, MAX_READ);
}

#[test]
fn a_full_pane_stops_after_the_first_step() {
    let line = |i: i32| format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"prompt {i}\"}},\"uuid\":\"{i}\"}}\n");
    let text = (0..40_000).map(line).collect::<Vec<_>>().concat();
    let len = u64::try_from(text.len()).unwrap();
    assert!(len > 2 * CHUNK);
    let load = read_chat(&mut Cursor::new(text.into_bytes()), len, 30, MAX_READ);
    assert_eq!(load.messages.len(), 30);
    assert_eq!(load.messages[29].text, "prompt 39999");
    assert_eq!(load.bytes_read, CHUNK);
}

#[test]
fn a_stream_hands_out_the_newest_message_after_one_chunk_then_older_ones_step_by_step() {
    // Given: five prompts, each followed by 100 KB of tool output.
    let filler = format!("{{\"type\":\"progress\",\"data\":\"{}\"}}\n", "x".repeat(1000)).repeat(100);
    let text: String = (0..5)
        .flat_map(|i| {
            [
                format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"prompt {i}\"}},\"uuid\":\"{i}\"}}\n"),
                filler.clone(),
            ]
        })
        .collect();
    let len = u64::try_from(text.len()).unwrap();
    let mut s = ChatStream::new(Cursor::new(text.into_bytes()), len, MAX_READ);
    // When: one chunk is read, nothing is found yet past the newest filler; then more chunks.
    let mut seen = Vec::new();
    while s.step() {
        seen.push(s.found());
    }
    // Then: each chunk read 64 KB at most, the messages came one by one, newest first.
    assert_eq!(seen.len(), usize::try_from(len.div_ceil(CHUNK)).unwrap());
    assert!(seen.windows(2).all(|w| w[1] - w[0] <= 1), "{seen:?}");
    assert_eq!(seen.iter().position(|n| *n == 1), Some(1), "prompt 4 sits 100 KB from the end");
    assert_eq!(s.messages().first().map(|m| m.text.as_str()), Some("prompt 0"));
    assert!(s.done());
}

#[test]
fn iso_stamps_parse_to_utc_ms() {
    assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(iso_ms("2026-09-16T12:00:00.000Z"), Some(1_789_560_000_000));
    assert_eq!(iso_ms("garbage"), None);
}
