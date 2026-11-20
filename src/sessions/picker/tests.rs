use super::*;
use crate::ui::PreviewTerm;
use crate::ui::screen_loop::run;
use std::collections::VecDeque;

fn row(id: &str, slug: &str, prompts: i64) -> Session {
    Session {
        session_id: id.into(),
        path: format!(r"C:\p\{slug}\{id}.jsonl").into(),
        slug: slug.into(),
        project: slug.into(),
        prompt_count: prompts,
        title: format!("title {id}"),
        ..Session::default()
    }
}

fn clock() -> Clock {
    Clock {
        now_ms: 1_789_560_000_000,
        zone: crate::sessions::frame::Zone(|_| 0),
    }
}

fn style() -> Style {
    Style { color: false, ascii: true }
}

fn keys(k: &[&str]) -> PreviewTerm {
    let q: VecDeque<KeyPress> = k.iter().map(|s| crate::input::scripted_key(s).unwrap()).collect();
    PreviewTerm::new((100, 30), q, true)
}

#[test]
fn overlap_is_not_the_end_and_offsets_count_fetched_rows() {
    let pages = [vec![row("a", "x", 1), row("b", "x", 1)], vec![row("b", "x", 1)], vec![]];
    let mut calls = Vec::new();
    let mut f = |have: usize, _s: &[String]| {
        calls.push(have);
        pages.get(calls.len() - 1).cloned().unwrap_or_default()
    };
    let mut b = Bucket::default();
    assert_eq!(expand_page(&mut b, &mut f, &[]), 2);
    assert_eq!(expand_page(&mut b, &mut f, &[]), 0, "an overlap page adds nothing");
    assert!(!b.exhausted, "but is not the end");
    assert_eq!(expand_page(&mut b, &mut f, &[]), 0);
    assert!(b.exhausted, "an empty page is");
    assert_eq!(calls, [0, 2, 3], "offsets count fetched rows, not the deduplicated list");
}

#[test]
fn enter_fork_and_escape() {
    let first = vec![row("a", "x", 1), row("b", "x", 2)];
    let mut s = PickerScreen::new(first.clone(), None, 0, &[], "", style(), clock());
    let got = run(&mut s, &mut keys(&["DownArrow", "Enter"])).unwrap().unwrap();
    assert_eq!((got.session.session_id.as_str(), got.fork), ("b", false));
    let mut s = PickerScreen::new(first.clone(), None, 0, &[], "", style(), clock());
    assert!(run(&mut s, &mut keys(&["f"])).unwrap().unwrap().fork);
    let mut s = PickerScreen::new(first, None, 0, &[], "", style(), clock());
    assert_eq!(run(&mut s, &mut keys(&["Escape"])), Ok(None));
}

#[test]
fn empty_sessions_are_never_offered_and_enter_on_nothing_continues() {
    let mut s = PickerScreen::new(vec![row("z", "x", 0)], None, 0, &[], "", style(), clock());
    assert_eq!(run(&mut s, &mut keys(&["Enter", "Escape"])), Ok(None));
}

#[test]
fn filter_box_swallows_hotkeys_and_escape_clears_it() {
    let first = vec![row("alpha", "x", 1), row("fbeta", "x", 1)];
    let mut s = PickerScreen::new(first.clone(), None, 0, &[], "", style(), clock());
    let got = run(&mut s, &mut keys(&["/", "f", "b", "Enter", "Enter"])).unwrap().unwrap();
    assert_eq!(
        (got.session.session_id.as_str(), got.fork),
        ("fbeta", false),
        "'f' typed into the filter never forks"
    );
    let mut s = PickerScreen::new(first, None, 0, &[], "", style(), clock());
    let got = run(&mut s, &mut keys(&["/", "b", "Escape", "Enter"]));
    assert_eq!(
        got.unwrap().unwrap().session.session_id,
        "alpha",
        "Esc clears the filter, not the picker"
    );
}

#[test]
fn scoped_picker_fills_pages_and_tab_widens_to_the_account() {
    let mine = ["m1", "m2", "m3"].map(|i| row(i, "mine", 1)).to_vec();
    let other = ["o1", "o2"].map(|i| row(i, "other", 1)).to_vec();
    let mut log: Vec<(usize, Vec<String>)> = Vec::new();
    let mut f = |have: usize, scope: &[String]| {
        log.push((have, scope.to_vec()));
        let src: Vec<Session> = if scope.is_empty() {
            [other.clone(), mine.clone()].concat()
        } else {
            mine.clone()
        };
        src.into_iter().skip(have).take(1).collect()
    };
    let first = f(0, &["mine".to_owned()]);
    let mut s = PickerScreen::new(first, Some(&mut f), 2, &["mine".to_owned()], "mine", style(), clock());
    let got = run(&mut s, &mut keys(&["DownArrow", "DownArrow", "Tab", "Enter"]))
        .unwrap()
        .unwrap();
    assert_eq!(got.session.session_id, "o1", "Tab opens the whole account at its first row");
    drop(s);
    assert!(log.iter().any(|(_, sc)| sc.is_empty()), "the account scope was fetched on Tab");
    assert!(
        log.iter().filter(|(_, sc)| !sc.is_empty()).count() >= 3,
        "the scoped pages kept coming on Down"
    );
}

/// A terminal resized by its script: `Resize` moves to the next size before it is delivered.
struct SizedTerm {
    inner: PreviewTerm,
    sizes: VecDeque<(usize, usize)>,
    size: (usize, usize),
    events: VecDeque<crate::input::Event>,
}

impl Term for SizedTerm {
    fn size(&mut self) -> (usize, usize) {
        self.size
    }
    fn paint(&mut self, lines: &[String]) {
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<crate::input::Event, crate::ui::UiStop> {
        let ev = self.events.pop_front().ok_or(crate::ui::UiStop::KeysExhausted)?;
        if matches!(ev, crate::input::Event::Resize) {
            self.size = self.sizes.pop_front().unwrap_or(self.size);
        }
        Ok(ev)
    }
    fn window_top(&mut self) -> i32 {
        0
    }
    fn input_pending(&mut self) -> bool {
        false
    }
    fn record_time(&self) -> Option<u64> {
        None
    }
    fn last_activation(&mut self) -> &mut Option<crate::ui::Activation> {
        self.inner.last_activation()
    }
    fn drain(&mut self) {}
    fn logger(&self) -> &crate::log::Logger {
        self.inner.logger()
    }
    fn read_line(&mut self, _prompt: &str) -> String {
        String::new()
    }
}

fn listed(frame: &[String]) -> usize {
    // A list row names its project (`x`) before the title; the preview card does not.
    frame.iter().filter(|l| l.contains("x  title s")).count()
}

#[test]
fn a_resize_redraws_at_the_new_size() {
    let first: Vec<Session> = (0..80).map(|i| row(&format!("s{i}"), "x", 1)).collect();
    let mut s = PickerScreen::new(first, None, 0, &[], "", style(), clock());
    let mut term = SizedTerm {
        inner: PreviewTerm::new((100, 30), VecDeque::new(), true),
        sizes: [(200, 60)].into(),
        size: (100, 30),
        events: [
            crate::input::Event::Resize,
            crate::input::Event::Key(crate::input::scripted_key("Escape").unwrap()),
        ]
        .into(),
    };
    assert_eq!(run(&mut s, &mut term), Ok(None));
    let frames = term.inner.frames.unwrap();
    let (small, big) = (&frames[0], &frames[frames.len() - 1]);
    assert_eq!((small.len(), big.len()), (29, 59), "each frame fills its terminal");
    assert!(big.iter().all(|l| crate::layout::display_width(l) <= 199));
    assert_eq!(crate::layout::display_width(&big[0]), 199);
    assert!(
        listed(big) > listed(small),
        "more rows listed: {} vs {}",
        listed(big),
        listed(small)
    );
}

#[test]
fn idle_polls_load_pages_until_a_tall_terminal_is_filled() {
    let all: Vec<Session> = (0..80).map(|i| row(&format!("s{i}"), "x", 1)).collect();
    let mut calls = 0;
    let mut f = |have: usize, _s: &[String]| {
        calls += 1;
        all.iter().skip(have).take(10).cloned().collect::<Vec<Session>>()
    };
    let first = f(0, &[]);
    let mut s = PickerScreen::new(first, Some(&mut f), 10, &[], "", style(), clock());
    let idle = std::iter::repeat_n(crate::input::Event::Idle, 8);
    let esc = crate::input::Event::Key(crate::input::scripted_key("Escape").unwrap());
    let mut term = crate::ui::script_term::ScriptTerm::new((100, 60), idle.chain([esc]));
    assert_eq!(run(&mut s, &mut term), Ok(None));
    drop(s);
    let frames = term.inner.frames.unwrap();
    assert_eq!(listed(&frames[0]), 10, "the first frame waits for one page only");
    assert!(listed(&frames[frames.len() - 1]) >= 55, "then idle polls fill the window");
    assert!(calls <= 8, "and stop once it is full: {calls} pages");
}

#[test]
fn a_click_selects_and_a_second_click_opens() {
    let first = vec![row("a", "x", 1), row("b", "x", 1)];
    let mut s = PickerScreen::new(first, None, 0, &[], "", style(), clock());
    s.before();
    assert_eq!(s.click(&Hit::ListRow(1)), Flow::Continue);
    s.before();
    assert_eq!(s.cursor.index, 1);
    assert!(matches!(s.activate(&Hit::ListRow(1)), Some(Flow::Done(Some(p))) if p.session.session_id == "b"));
    assert_eq!(s.click(&Hit::ListRow(9)), Flow::Continue);
    assert_eq!(s.cursor.index, 1, "a click past the end moves nothing");
}

#[test]
fn without_a_chat_pane_the_summary_messages_show_as_plain_text() {
    use crate::sessions::listing::{Message, Speaker};
    let mut a = row("a", "x", 1);
    a.recent = vec![Message {
        speaker: Speaker::Assistant,
        text: "**Done:** shipped `v1`".into(),
    }];
    let mut s = PickerScreen::new(vec![a], None, 0, &[], "", style(), clock());
    let mut t = keys(&[]);
    assert_eq!(run(&mut s, &mut t), Err(crate::ui::UiStop::KeysExhausted));
    let frame = &t.frames.unwrap()[0];
    assert!(frame.iter().any(|l| l.contains("Done: shipped v1")), "{frame:#?}");
}

/// AC3: the UI thread never waits on a chat load. The loader below is held until the run ends.
#[test]
fn a_key_draws_its_frame_while_a_chat_load_is_held() {
    use crate::input::Event;
    use crate::sessions::chat::ChatLoad;
    use crate::sessions::chat::pane::{ChatPane, Loader};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    // Given: a loader that reports its start, then blocks until released.
    let (release, held) = channel::<()>();
    let (started_tx, started) = channel::<()>();
    let (held, started_tx) = (Mutex::new(held), Mutex::new(started_tx));
    let loader: Loader = Arc::new(move |_p, _want| {
        started_tx.lock().unwrap().send(()).unwrap();
        held.lock().unwrap().recv().unwrap_or_default();
        ChatLoad::default()
    });
    let first = vec![row("a", "x", 1), row("b", "x", 1), row("c", "x", 1)];
    let mut s =
        PickerScreen::new(first, None, 0, &[], "", style(), clock()).with_chat(ChatPane::new(loader, crate::timing::Timings::default()));
    let down = || Event::Key(KeyPress::named(vk::DOWN));
    let mut t = crate::ui::script_term::ScriptTerm::new((100, 30), [Event::Idle, Event::Idle, down(), down()]);
    // When: two idle polls start the load, then two keys come while it is held.
    assert_eq!(run(&mut s, &mut t), Err(crate::ui::UiStop::KeysExhausted));
    // Then: the load started and is still held, yet each key drew its frame, the pane loading.
    started.recv().unwrap();
    let frames = t.inner.frames.clone().unwrap();
    assert_eq!(frames.len(), 3, "the opening frame and one per key");
    assert!(frames.iter().all(|f| f.iter().any(|l| l.contains("loading..."))), "{frames:#?}");
    release.send(()).unwrap();
}
