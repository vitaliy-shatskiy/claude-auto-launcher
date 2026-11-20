use super::*;
use crate::frame::Style;
use crate::sessions::frame::Clock;
use crate::sessions::listing::{cache_keys, cache_path, session_files};
use crate::sessions::picker::PickerScreen;
use crate::ui::screen_loop::Screen;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// One transcript whose first prompt differs from the title only a full scan finds.
fn world(tag: &str) -> (Tmp, PathBuf) {
    let base = std::env::temp_dir().join(format!("cars-s1-{tag}-{}", std::process::id()));
    if base.exists() {
        std::fs::remove_dir_all(&base).unwrap();
    }
    let dir = base.join("projects").join("C--src-demo");
    std::fs::create_dir_all(&dir).unwrap();
    let text = [
        r#"{"type":"user","message":{"role":"user","content":"quick first ask"}}"#,
        r#"{"type":"ai-title","aiTitle":"scanned full title","sessionId":"s1"}"#,
    ]
    .join("\n")
        + "\n";
    std::fs::write(dir.join("s1.jsonl"), text).unwrap();
    let root = base.join("projects");
    (Tmp(base), root)
}

/// A full summary held until the test sends on the returned gate; the stub calls the real one.
fn held() -> (Summarize, Sender<()>) {
    let (gate, wait) = channel::<()>();
    let wait = Mutex::new(wait);
    let full: Summarize = Arc::new(move |e: &Entry| {
        wait.lock().ok()?.recv().ok()?;
        e.summarize()
    });
    (full, gate)
}

fn text(screen: &mut PickerScreen<'_>) -> String {
    screen.before();
    screen.draw((100, 30)).lines.join("\n")
}

fn clock() -> Clock {
    Clock {
        now_ms: 1_789_560_000_000,
        offset_minutes: 0,
    }
}

/// The picker over `root` as `run_picker_with` builds it off preview, with `landed` as its feed.
fn picker(fetch: crate::sessions::picker::Fetch<'_>, landed: Receiver<Vec<Session>>) -> PickerScreen<'_> {
    let first = fetch(0, &[]);
    PickerScreen::new(first, Some(fetch), 10, &[], "", Style { color: false, ascii: true }, clock()).with_landed(landed)
}

#[test]
fn the_first_frame_shows_quick_rows_while_the_full_scan_is_held() {
    let (_t, root) = world("held");
    let cache_file = cache_path(&root);
    let (full, gate) = held();
    let (done, _results) = channel();
    let worker = Summarizer::start(full, done);
    let snap = session_files(&root, &[]);
    let mut cache = SessionCache::new(false);
    let mut fetch = |have: usize, _: &[String]| worker.page(&mut cache, &root, &snap, have, 10, &cache_file);
    let (_feed, landed) = channel();
    let mut screen = picker(&mut fetch, landed);
    let frame = text(&mut screen);
    assert!(frame.contains("quick first ask"), "the quick row is listed at once:\n{frame}");
    assert!(
        !frame.contains("scanned full title"),
        "no full scan before the first frame:\n{frame}"
    );
    assert_eq!(cache_keys(&cache_file), 0, "a quick row is never stored");
    drop(gate);
}

#[test]
fn rows_take_the_full_summary_in_place_when_it_lands_and_the_cache_keeps_it() {
    let (_t, root) = world("landed");
    let cache_file = cache_path(&root);
    let (full, gate) = held();
    let (done, results) = channel();
    let worker = Summarizer::start(full, done);
    let snap = session_files(&root, &[]);
    let mut cache = SessionCache::new(false);
    let mut fetch = |have: usize, _: &[String]| worker.page(&mut cache, &root, &snap, have, 10, &cache_file);
    let (feed, landed) = channel();
    let mut screen = picker(&mut fetch, landed);
    text(&mut screen);
    assert!(!screen.idle(), "nothing landed yet");
    gate.send(()).unwrap();
    let batch = results.recv().unwrap();
    assert_eq!(cache_keys(&cache_file), 1, "the full summary is in the cache file before it lands");
    feed.send(batch).unwrap();
    assert!(screen.idle(), "a landed batch asks for one redraw");
    let frame = text(&mut screen);
    assert!(frame.contains("scanned full title"), "the row shows the full summary:\n{frame}");
    assert!(!screen.idle(), "and only one");
}
