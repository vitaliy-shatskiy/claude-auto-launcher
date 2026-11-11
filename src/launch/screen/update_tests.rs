//! The launch screen over a slow update check: the first frame never waits for it, its answer
//! redraws the header and the model row in place, the selection stays.

use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::Sender;

use super::*;
use crate::input::Event;
use crate::launch::tests::four;
use crate::maintenance::watch::tests::{found, slow_check};
use crate::ui::screen_loop::run;
use crate::ui::{PreviewTerm, UiStop};

fn view() -> LaunchView {
    LaunchView {
        limits: Limits::new(),
        version: v("2.1.1"),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: BTreeMap::new(),
        style: Style { color: false, ascii: true },
    }
}

fn v(installed: &str) -> VersionInfo {
    VersionInfo {
        installed: Some(installed.into()),
        newest: None,
    }
}

/// Keys, then one idle poll that lets the check answer, then idle polls until the screen paints
/// again (at most `cap`, so a screen that never redraws ends the wait), then the rest.
struct UpdateTerm {
    inner: PreviewTerm,
    before: VecDeque<Event>,
    go: Option<Sender<()>>,
    painted: usize,
    cap: usize,
    after: VecDeque<Event>,
}

impl UpdateTerm {
    fn new(before: &str, go: Sender<()>, cap: usize, after: &str) -> Self {
        let keys = |k: &str| crate::ui::parse_preview_keys(k).unwrap().into_iter().map(Event::Key).collect();
        Self {
            inner: PreviewTerm::new((100, 30), VecDeque::new(), true),
            before: keys(before),
            go: Some(go),
            painted: 0,
            cap,
            after: keys(after),
        }
    }

    fn frames(&self) -> &[Vec<String>] {
        self.inner.frames.as_deref().unwrap()
    }
}

impl Term for UpdateTerm {
    fn size(&mut self) -> (usize, usize) {
        (100, 30)
    }
    fn paint(&mut self, lines: &[String]) {
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<Event, UiStop> {
        if let Some(e) = self.before.pop_front() {
            return Ok(e);
        }
        if let Some(go) = self.go.take() {
            go.send(()).unwrap();
            self.painted = self.frames().len();
            return Ok(Event::Idle);
        }
        if self.frames().len() == self.painted && self.cap > 0 {
            self.cap -= 1;
            return Ok(Event::Idle);
        }
        self.after.pop_front().ok_or(UiStop::KeysExhausted)
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

fn haiku_label(label: &str) -> Option<BTreeMap<String, String>> {
    Some(BTreeMap::from([("haiku".to_owned(), label.to_owned())]))
}

fn has(frame: &[String], text: &str) -> bool {
    frame.iter().any(|l| l.contains(text))
}

#[test]
fn the_first_frame_never_waits_for_the_update_check() {
    // Given: a check that answers only when the test lets it.
    let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), haiku_label("Haiku 9.9")));
    let (mut r, vw, p) = (four(), view(), Prefs::default());
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("Enter").unwrap(), true);
    // When: the screen runs to Enter while the check is still out.
    let out = {
        let mut s = LaunchScreen::new(&mut r, st, &vw, &p, 0, &mut hook).with_update(&mut w);
        run(&mut s, &mut t).unwrap()
    };
    // Then: it drew and ended with what was known at start.
    assert!(out.is_some());
    let first = t.frames.unwrap().into_iter().next().unwrap();
    assert!(has(&first, "v2.1.1") && has(&first, "Haiku 5.5"), "{first:#?}");
    go.send(()).unwrap();
}

#[test]
fn the_answer_redraws_header_and_model_row_and_keeps_the_selection() {
    let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), haiku_label("Haiku 9.9")));
    let (mut r, vw, p) = (four(), view(), Prefs::default());
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    // Given: haiku picked on the model row while the check runs.
    let mut t = UpdateTerm::new("DownArrow,DownArrow,RightArrow", go, 1_000_000, "Enter");
    // When: the check answers a new version and new labels.
    let out = {
        let mut s = LaunchScreen::new(&mut r, st, &vw, &p, 0, &mut hook).with_update(&mut w);
        run(&mut s, &mut t).unwrap().unwrap()
    };
    // Then: one more frame shows them, and the pick launches unchanged.
    let last = t.frames().last().unwrap();
    assert!(has(last, "v2.1.2") && has(last, "Haiku 9.9"), "{last:#?}");
    assert_eq!(t.frames().len(), t.painted + 1, "exactly one redraw for the answer");
    assert_eq!(out.model, "haiku");
    assert_eq!(out.row, 2);
}

#[test]
fn an_answer_that_changes_nothing_draws_nothing() {
    let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.1"), None));
    let (mut r, vw, p) = (four(), view(), Prefs::default());
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut t = UpdateTerm::new("", go, 1_000_000, "Enter");
    {
        let mut s = LaunchScreen::new(&mut r, st, &vw, &p, 0, &mut hook).with_update(&mut w);
        run(&mut s, &mut t).unwrap().unwrap();
    }
    assert_eq!(t.frames().len(), 1, "only the first frame");
    // The check did answer: the idle polls saw it and drew nothing.
    w.finish(|| {});
    assert!(matches!(w.outcome(), Some(Ok(_))));
}

#[test]
fn an_answer_already_in_shows_in_the_first_frame() {
    // An update that finished while the project screen was open shows on return.
    let (go, mut w) = slow_check(v("2.1.1"), found(v("2.1.2"), None));
    go.send(()).unwrap();
    w.finish(|| {});
    let (mut r, vw, p) = (four(), view(), Prefs::default());
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = r.new_state();
    let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("Enter").unwrap(), true);
    {
        let mut s = LaunchScreen::new(&mut r, st, &vw, &p, 0, &mut hook).with_update(&mut w);
        run(&mut s, &mut t).unwrap();
    }
    assert!(has(&t.frames.unwrap()[0], "v2.1.2"));
}
