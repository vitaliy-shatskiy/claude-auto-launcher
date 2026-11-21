use super::*;
use crate::sessions::chat::{CHUNK, MAX_READ};
use std::fmt::Write as _;
use std::path::Path;

/// A temp folder of synthetic transcripts, removed at the end.
struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

fn tmp(name: &str) -> Tmp {
    let d = std::env::temp_dir().join(format!("cars-chat-pane-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    Tmp(d)
}

/// `turns` exchanges, each followed by `filler` bytes of tool output (no chat line).
fn transcript(turns: usize, filler: usize) -> String {
    let tool = format!("{{\"type\":\"progress\",\"data\":\"{}\"}}\n", "x".repeat(990));
    let mut s = String::new();
    for n in 0..turns {
        writeln!(
            s,
            "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"question {n}\"}},\"uuid\":\"u{n}\"}}"
        )
        .unwrap();
        s.push_str(&tool.repeat(filler / tool.len()));
        writeln!(
            s,
            "{{\"type\":\"assistant\",\"uuid\":\"a{n}\",\"message\":{{\"id\":\"m{n}\",\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"answer {n}\"}}]}}}}"
        )
        .unwrap();
    }
    s
}

fn key_of(path: &Path) -> ChatKey {
    ChatKey {
        path: path.to_path_buf(),
        size: std::fs::metadata(path).unwrap().len(),
        modified_ms: 1,
    }
}

/// Idle polls until the workers have nothing left; returns how many said "redraw".
fn settle(p: &mut ChatPane) -> usize {
    let mut redraws = 0;
    while !p.quiet() {
        redraws += usize::from(p.idle());
        std::thread::yield_now();
    }
    redraws
}

const PANE: Fill = Fill { rows: 20, cols: 80 };

#[test]
fn a_stream_fills_the_pane_plus_a_screen_and_a_revisit_reads_nothing() {
    // Given: a 700 KB transcript of short exchanges between tool output.
    let t = tmp("fill");
    let f = t.0.join("a.jsonl");
    std::fs::write(&f, transcript(300, 2000)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    // When: the session is highlighted and the cursor rests.
    assert_eq!(p.view(&key_of(&f), PANE), ChatState::Loading);
    assert!(settle(&mut p) >= 1, "the arrival asks a redraw");
    // Then: one chunk filled 40 rows (the pane plus one screen), newest last.
    assert_eq!(p.bytes_read(), CHUNK);
    let ChatState::Ready(m) = p.view(&key_of(&f), PANE) else {
        panic!("ready")
    };
    assert!(m.len() >= 12, "{} messages", m.len());
    assert_eq!(m.last().map(|m| m.text.as_str()), Some("answer 299"));
    // And: going back to it reads nothing.
    assert_eq!(p.view(&key_of(&f), PANE), ChatState::Ready(m));
    for _ in 0..5 {
        assert!(!p.idle());
    }
    assert_eq!(p.bytes_read(), CHUNK, "a revisit reads 0 bytes");
}

#[test]
fn a_grown_transcript_shows_the_old_chat_updating_then_streams_only_its_new_tail() {
    let t = tmp("grown");
    let f = t.0.join("g.jsonl");
    std::fs::write(&f, transcript(300, 2000)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    p.view(&key_of(&f), PANE);
    settle(&mut p);
    // When: the transcript grows by one exchange.
    let mut grown = std::fs::read_to_string(&f).unwrap();
    grown.push_str(
        &transcript(1, 0)
            .replace("question 0", "late question")
            .replace("answer 0", "late answer"),
    );
    std::fs::write(&f, grown).unwrap();
    // Then: the old chat shows at once, marked updating, and the new one streams one chunk.
    assert!(matches!(p.view(&key_of(&f), PANE), ChatState::Updating(_)));
    settle(&mut p);
    let ChatState::Ready(m) = p.view(&key_of(&f), PANE) else {
        panic!("ready")
    };
    assert_eq!(m.last().map(|m| m.text.as_str()), Some("late answer"));
    assert_eq!(p.bytes_read(), 2 * CHUNK, "the bounded tail, never the whole file");
}

#[test]
fn a_transcript_of_tool_output_only_stops_at_the_cap() {
    let t = tmp("cap");
    let f = t.0.join("c.jsonl");
    std::fs::write(&f, transcript(1, 9 << 20)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    p.view(&key_of(&f), PANE);
    settle(&mut p);
    assert_eq!(p.bytes_read(), MAX_READ);
    let ChatState::Ready(m) = p.view(&key_of(&f), PANE) else {
        panic!("ready")
    };
    assert_eq!(m.len(), 1, "the one message in reach");
}

#[test]
fn prefetch_waits_for_idle_polls_reads_one_chunk_per_poll_and_stops_at_its_cap() {
    // Given: two transcripts of tool output only (nothing fills their pane).
    let t = tmp("prefetch");
    let (a, b) = (t.0.join("a.jsonl"), t.0.join("b.jsonl"));
    std::fs::write(&a, transcript(1, 3 << 20)).unwrap();
    std::fs::write(&b, transcript(1, 3 << 20)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    p.prefetch(vec![key_of(&a), key_of(&b)], PANE);
    // Then: no idle poll (input pending), no read.
    assert!(p.prefetching.is_none());
    assert_eq!(p.bytes_read(), 0);
    // And: the cursor's rest comes first, then one chunk per grant.
    assert!(!p.idle());
    assert!(p.prefetching.is_none(), "one poll is no rest yet");
    p.idle();
    assert_eq!(p.prefetching, Some(key_of(&a)));
    settle(&mut p);
    // Then: each session read up to the prefetch cap, no further.
    assert_eq!(p.bytes_read(), 2 * PREFETCH_BYTES);
}

#[test]
fn prefetch_yields_to_the_highlighted_session() {
    let t = tmp("yield");
    let (a, b) = (t.0.join("a.jsonl"), t.0.join("b.jsonl"));
    std::fs::write(&a, transcript(300, 2000)).unwrap();
    std::fs::write(&b, transcript(300, 2000)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    p.prefetch(vec![key_of(&b)], PANE);
    p.view(&key_of(&a), PANE);
    p.idle();
    p.idle();
    assert_eq!(p.in_flight.as_ref().map(|(k, _)| k.clone()), Some(key_of(&a)));
    assert!(p.prefetching.is_none(), "no prefetch while the cursor's session streams");
    settle(&mut p);
    assert!(matches!(p.view(&key_of(&a), PANE), ChatState::Ready(_)));
    assert_eq!(p.bytes_read(), 2 * CHUNK, "a's fill, then b's one prefetch chunk");
}

#[test]
fn both_workers_end_when_the_picker_closes() {
    let t = tmp("close");
    let f = t.0.join("a.jsonl");
    std::fs::write(&f, transcript(1, 3 << 20)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    p.prefetch(vec![key_of(&f)], PANE);
    p.idle();
    p.idle();
    for h in p.close() {
        h.join().unwrap();
    }
}

#[test]
fn scrolling_fifty_sessions_keeps_at_most_sixteen_streams_open_the_viewed_one_among_them() {
    // Given: 50 transcripts, each longer than the chunk that fills the pane, and one short one.
    let t = tmp("lru");
    let keys: Vec<ChatKey> = (0..50)
        .map(|i| {
            let f = t.0.join(format!("s{i}.jsonl"));
            std::fs::write(&f, transcript(100, 2000)).unwrap();
            key_of(&f)
        })
        .collect();
    let short = t.0.join("short.jsonl");
    std::fs::write(&short, transcript(2, 0)).unwrap();
    let mut p = ChatPane::new(Timings::default());
    // When: the cursor rests on each in turn, the short one first.
    p.view(&key_of(&short), PANE);
    settle(&mut p);
    assert_eq!(p.open_streams(), 0, "a stream read to the file's start is closed");
    for k in &keys {
        p.view(k, PANE);
        settle(&mut p);
    }
    // Then: the oldest streams were closed, the viewed one is still open, its chat still shown.
    assert_eq!(p.open_streams(), MAX_OPEN);
    assert!(p.is_open(keys.last().unwrap()));
    assert!(matches!(p.view(&keys[0], PANE), ChatState::Ready(m) if !m.is_empty()));
}
