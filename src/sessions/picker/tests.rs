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
        offset_minutes: 0,
    }
}

fn style() -> Style {
    Style { color: false, ascii: true }
}

fn keys(k: &[&str]) -> PreviewTerm {
    let q: VecDeque<KeyPress> = k.iter().map(|s| crate::input::scripted_key(s).unwrap()).collect();
    PreviewTerm::new((100, 30), q, true)
}

fn picker_frames(timings: crate::timing::Timings) -> Vec<Vec<String>> {
    let first = vec![row("a", "x", 1), row("b", "x", 2)];
    let mut s = PickerScreen::new(first, None, 0, &[], "", style(), clock());
    let mut t = keys(&["DownArrow", "Enter"]).with_logger(crate::log::Logger::silent(timings));
    assert!(run(&mut s, &mut t).unwrap().is_some());
    t.frames.unwrap()
}

#[test]
fn debug_mode_draws_the_picker_exactly_as_off() {
    let on = crate::timing::Timings::on(std::time::Instant::now());
    assert_eq!(picker_frames(on), picker_frames(crate::timing::Timings::default()));
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
