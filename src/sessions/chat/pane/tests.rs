use super::*;
use crate::sessions::listing::Speaker;
use std::sync::Mutex;
use std::sync::mpsc::Receiver as Rx;

fn key(name: &str) -> ChatKey {
    ChatKey {
        path: PathBuf::from(format!(r"C:\p\{name}.jsonl")),
        size: 10,
        modified_ms: 1,
    }
}

fn one(text: &str) -> ChatLoad {
    ChatLoad {
        messages: vec![ChatMessage {
            speaker: Speaker::User,
            text: text.into(),
            at_ms: None,
        }],
        bytes_read: 10,
    }
}

/// A loader that names each load on `calls` and answers the path's file stem.
fn counting() -> (Loader, Rx<PathBuf>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let loader: Loader = Arc::new(move |p: &Path, _want: usize| {
        tx.lock().unwrap().send(p.to_path_buf()).unwrap();
        one(&p.file_stem().unwrap().to_string_lossy())
    });
    (loader, rx)
}

/// Polls until the worker's answer lands; returns how many polls said "redraw".
fn settle(p: &mut ChatPane) -> usize {
    let mut redraws = 0;
    while p.in_flight.is_some() {
        redraws += usize::from(p.idle());
        std::thread::yield_now();
    }
    redraws
}

#[test]
fn a_load_starts_after_the_cursor_rests_and_its_arrival_asks_one_redraw() {
    // Given: a pane with nothing cached.
    let (loader, calls) = counting();
    let mut p = ChatPane::new(loader, Timings::default());
    // When: the highlighted session is shown, then idle polls come.
    assert_eq!(p.view(&key("a"), 10), ChatState::Loading);
    assert!(!p.idle(), "one poll is no rest yet");
    assert!(p.in_flight.is_none());
    assert!(!p.idle());
    assert_eq!(p.in_flight, Some(key("a")), "the second poll starts the load");
    // Then: its arrival is one redraw, and the next view is the chat.
    assert_eq!(settle(&mut p), 1);
    assert!(!p.idle(), "no second redraw");
    assert_eq!(p.view(&key("a"), 10), ChatState::Ready(Arc::new(one("a").messages)));
    assert_eq!(calls.try_iter().count(), 1);
}

#[test]
fn a_cursor_that_keeps_moving_loads_nothing_and_a_cached_chat_needs_no_load() {
    let (loader, calls) = counting();
    let mut p = ChatPane::new(loader, Timings::default());
    for name in ["a", "b", "c"] {
        assert_eq!(p.view(&key(name), 10), ChatState::Loading);
        assert!(!p.idle());
    }
    assert!(p.in_flight.is_none(), "every view restarted the rest");
    assert!(!p.idle());
    settle(&mut p);
    assert_eq!(calls.try_iter().collect::<Vec<_>>(), [key("c").path]);
    assert!(matches!(p.view(&key("c"), 10), ChatState::Ready(_)));
    assert!(!p.idle());
    assert!(!p.idle());
    assert!(p.in_flight.is_none(), "a cached chat starts no load");
}

#[test]
fn a_grown_transcript_is_another_key() {
    let (loader, _calls) = counting();
    let mut p = ChatPane::new(loader, Timings::default());
    p.view(&key("a"), 10);
    p.idle();
    p.idle();
    settle(&mut p);
    let grown = ChatKey { size: 20, ..key("a") };
    assert_eq!(p.view(&grown, 10), ChatState::Loading);
}

#[test]
fn the_cache_keeps_the_newest_thirty_two() {
    let (loader, _calls) = counting();
    let mut p = ChatPane::new(loader, Timings::default());
    for i in 0..33 {
        p.insert(key(&i.to_string()), Vec::new());
    }
    assert_eq!(p.cache.len(), CACHE_SIZE);
    assert_eq!(p.view(&key("0"), 10), ChatState::Loading, "the oldest went");
    assert!(matches!(p.view(&key("32"), 10), ChatState::Ready(_)));
}
