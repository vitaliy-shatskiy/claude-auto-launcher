use super::*;

#[test]
fn prefixes_are_shortest_unique() {
    let p = account_prefixes(&["work", "main", "mamoru", "ma"]);
    assert_eq!(p.get("work").map(String::as_str), Some("w"));
    assert_eq!(p.get("main").map(String::as_str), Some("mai"));
    assert_eq!(p.get("mamoru").map(String::as_str), Some("mam"));
    assert_eq!(
        p.get("ma").map(String::as_str),
        Some("ma"),
        "a key that prefixes another keeps itself"
    );
}

#[test]
fn f12_on_the_held_session_warning_shows_the_timings_then_redraws_it() {
    let logger = Logger {
        root: PathBuf::new(),
        run_id: String::new(),
        enabled: false,
        timings: Timings::on(Instant::now()),
    };
    let keys = crate::ui::parse_preview_keys("F12,Escape,Enter").unwrap();
    let mut t = crate::ui::PreviewTerm::new((80, 24), keys, true).with_logger(logger);
    let style = crate::frame::Style { color: false, ascii: true };
    assert_eq!(screens::confirm_held(&mut t, Some("held by pid 1"), style), Ok(true));
    let frames = t.frames.unwrap();
    assert_eq!(frames.len(), 3, "warning, timings, warning again");
    assert!(frames[1].iter().any(|l| l.contains("timings of this run")));
    assert!(frames[2].iter().any(|l| l.contains("enter")));
    assert_eq!(frames[2], frames[0], "the warning is drawn again as it was");
}

fn held_frames(timings: Timings) -> Vec<Vec<String>> {
    let keys = crate::ui::parse_preview_keys("Escape").unwrap();
    let mut t = crate::ui::PreviewTerm::new((80, 24), keys, true).with_logger(Logger::silent(timings));
    let style = crate::frame::Style { color: false, ascii: true };
    assert_eq!(screens::confirm_held(&mut t, Some("held by pid 1"), style), Ok(false));
    t.frames.unwrap()
}

#[test]
fn debug_mode_draws_the_held_warning_exactly_as_off() {
    assert_eq!(held_frames(Timings::on(Instant::now())), held_frames(Timings::default()));
}

/// Scripted events, mouse included, for the held warning.
struct HeldEvents {
    inner: crate::ui::PreviewTerm,
    q: std::collections::VecDeque<crate::input::Event>,
}

impl crate::ui::Term for HeldEvents {
    fn size(&mut self) -> (usize, usize) {
        (80, 24)
    }
    fn paint(&mut self, lines: &[String]) {
        self.inner.paint(lines);
    }
    fn wait(&mut self, _s: (usize, usize)) -> Result<crate::input::Event, crate::ui::UiStop> {
        self.q.pop_front().ok_or(crate::ui::UiStop::KeysExhausted)
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
    fn logger(&self) -> &Logger {
        self.inner.logger()
    }
    fn read_line(&mut self, _prompt: &str) -> String {
        String::new()
    }
}

/// Clicks the held warning's footer button `label` after one empty draw found where it sits.
fn click_held(label: &str) -> Result<bool, crate::ui::UiStop> {
    use crate::input::{Event, MouseEvent};
    let style = crate::frame::Style { color: false, ascii: true };
    let mut probe = crate::ui::PreviewTerm::new((80, 24), crate::ui::parse_preview_keys("Escape").unwrap(), true);
    assert_eq!(screens::confirm_held(&mut probe, Some("held by pid 1"), style), Ok(false));
    let frame = probe.frames.unwrap().remove(0);
    let (y, line) = frame.iter().enumerate().rev().find(|(_, l)| l.contains(label)).unwrap();
    let x = line.find(label).unwrap();
    let press = Event::Mouse(MouseEvent {
        x: i32::try_from(x).unwrap(),
        y: i32::try_from(y).unwrap(),
        left: true,
        buttons: 1,
        ..MouseEvent::default()
    });
    let mut t = HeldEvents {
        inner: crate::ui::PreviewTerm::new((80, 24), std::collections::VecDeque::default(), true),
        q: [press].into(),
    };
    screens::confirm_held(&mut t, Some("held by pid 1"), style)
}

#[test]
fn the_held_warning_has_go_on_and_back_buttons() {
    let frame = &held_frames(Timings::default())[0];
    let foot = frame.last().unwrap();
    assert!(foot.contains("[enter] go on") && foot.contains("[esc] back"), "{foot}");
    assert_eq!(click_held("[enter] go on"), Ok(true));
    assert_eq!(click_held("[esc] back"), Ok(false));
}

#[test]
fn a_click_off_the_held_buttons_does_nothing() {
    use crate::input::{Event, MouseEvent};
    let press = Event::Mouse(MouseEvent {
        x: 4,
        y: 1,
        left: true,
        buttons: 1,
        ..MouseEvent::default()
    });
    let mut t = HeldEvents {
        inner: crate::ui::PreviewTerm::new((80, 24), std::collections::VecDeque::default(), true),
        q: [press, Event::Key(crate::input::KeyPress::named(crate::input::vk::ENTER))].into(),
    };
    let style = crate::frame::Style { color: false, ascii: true };
    assert_eq!(
        screens::confirm_held(&mut t, Some("held by pid 1"), style),
        Ok(true),
        "only enter ended it"
    );
}

#[test]
fn on_demand_enter_skips_project_screen() {
    use crate::config::ProjectScreen;
    assert!(!screens::opens_project_screen(ProjectScreen::OnDemand, "new"), "Enter at once");
    assert!(!screens::opens_project_screen(ProjectScreen::OnDemand, "continue"));
    assert!(
        screens::opens_project_screen(ProjectScreen::OnDemand, "project"),
        "the project action opens it"
    );
    assert!(screens::opens_project_screen(ProjectScreen::Always, "new"), "always: today's flow");
    let mut s = LaunchState::fresh("work");
    s.project = r"C:\remembered".into();
    let p = crate::projects::Project {
        name: "here".into(),
        path: r"C:\here".into(),
        slug: "C--here".into(),
        slugs: vec!["C--here".into(), "C--here-alt".into()],
        worktree: None,
        last_activity_ms: None,
        hidden: false,
    };
    screens::start_in_cwd(&mut s, Path::new(r"C:\here"), &[p]);
    assert_eq!(
        (s.project.as_str(), s.project_slug.as_str(), s.project_slugs.len()),
        (r"C:\here", "C--here", 2),
        "the launch directory, with every slug, never the remembered project"
    );
}

#[test]
fn mouse_false_arms_nothing() {
    assert!(!screens::mouse_armed(false, None), "config mouse false");
    assert!(!screens::mouse_armed(true, Some("1")), "the variable still wins");
    assert!(screens::mouse_armed(true, None), "on by default");
    assert!(screens::mouse_armed(true, Some("0")));
}

#[test]
fn answers_resolve_by_full_key_or_longest_prefix() {
    let p = account_prefixes(&["work", "personal"]);
    assert_eq!(resolve_account_answer(" Personal ", &p, "work"), "personal");
    assert_eq!(resolve_account_answer("p", &p, "work"), "personal");
    assert_eq!(resolve_account_answer("", &p, "work"), "work");
    assert_eq!(resolve_account_answer("x", &p, "work"), "work");
}

#[test]
fn a_tab_switch_then_a_neutral_cwd_starts_in_the_arriving_tabs_remembered_project() {
    use crate::launch::screen::{LaunchScreen, LaunchView};
    use crate::ui::{PreviewTerm, Term};
    let dir = std::env::temp_dir().join(format!("claude-auto-rs-tab-{}", crate::log::new_run_id()));
    let (pa, pb) = (dir.join("alpha"), dir.join("beta"));
    std::fs::create_dir_all(&pa).unwrap();
    std::fs::create_dir_all(&pb).unwrap();
    // Given: tab work remembers alpha, tab personal beta; the launcher runs outside any project.
    let prefs = crate::prefs::parse_prefs(
        &serde_json::json!({ "Version": 2, "Profiles": { "personal": { "Project": pb.to_string_lossy(), "SavedAtMs": 0 } } }).to_string(),
    );
    let mut r = crate::launch::tests::four();
    let mut st = r.new_state();
    st.project = pa.to_string_lossy().into_owned();
    let view = LaunchView {
        limits: crate::types::Limits::new(),
        version: crate::types::VersionInfo::default(),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: std::collections::BTreeMap::new(),
        style: crate::frame::Style { color: false, ascii: true },
    };
    let mut hook = |_: &crate::input::KeyPress, _: &mut dyn Term| false;
    let mut t = PreviewTerm::new((100, 30), crate::ui::parse_preview_keys("RightArrow,Enter").unwrap(), true);
    // When: the account row moves to personal, Enter, then the deferred start rule runs.
    let mut state = {
        let mut s = LaunchScreen::new(&mut r, st, &view, &prefs, 0, &mut hook);
        crate::ui::screen_loop::run(&mut s, &mut t).unwrap().unwrap()
    };
    let source = crate::projects::set_launch_start_project(&mut state, Path::new(r"C:\Users\someone\Desktop"), &[]);
    // Then: the final tab's project, recorded as remembered.
    assert_eq!(state.account, "personal");
    assert_eq!((state.project.as_str(), source.as_str()), (pb.to_str().unwrap(), "remembered"));
    std::fs::remove_dir_all(&dir).unwrap_or_default();
}

#[test]
fn maintenance_opens_only_after_the_background_check() {
    use crate::maintenance::watch::tests::found;
    use crate::maintenance::watch::tests::slow_install;
    use crate::timing::{DebugMode, Timings};
    use crate::types::VersionInfo;
    let tm = Timings::new(DebugMode::Quiet, std::time::Instant::now());
    // Given: the check's own `claude update` is running when `u` is pressed.
    let (started, go, w) = slow_install(found(VersionInfo::default(), None));
    assert!(started.recv().unwrap());
    let update = std::cell::RefCell::new(w);
    let release = std::thread::spawn(move || go.send(()).unwrap());
    let mut t = crate::ui::PreviewTerm::new((80, 25), std::collections::VecDeque::new(), true);
    let mut opened_after_check = None;
    // When: maintenance opens.
    screens::open_maintenance(&update, &tm, &mut t, true, |_| {
        opened_after_check = Some(update.borrow().outcome().is_some());
    });
    // Then: the install had finished before any maintenance key could run another.
    assert_eq!(opened_after_check, Some(true));
    release.join().unwrap();
}

#[test]
fn a_launch_waits_for_an_unfinished_update_check_and_a_cancel_does_not() {
    use crate::maintenance::watch::tests::{found, slow_check};
    use crate::timing::{DebugMode, Timings};
    use crate::types::VersionInfo;
    let tm = Timings::new(DebugMode::Quiet, std::time::Instant::now());
    // Given: checks still talking to the network when the launch screen ends.
    let (go_quit, mut quit) = slow_check(VersionInfo::default(), found(VersionInfo::default(), None));
    let (go, mut w) = slow_check(VersionInfo::default(), found(VersionInfo::default(), None));
    // When: a cancel ends the UI, nothing waits.
    let release_quit = std::thread::spawn(move || go_quit.send(()).unwrap());
    screens::wait_for_update(false, &mut quit, &tm, || {});
    assert!(quit.outcome().is_none(), "the cancel did not read the check");
    release_quit.join().unwrap();
    // When: Enter launches while it is still running.
    let release = std::thread::spawn(move || go.send(()).unwrap());
    screens::wait_for_update(true, &mut w, &tm, || {});
    // Then: the check answered before claude would start, and the wait is a span.
    assert!(matches!(w.outcome(), Some(Ok(_))));
    assert!(tm.spans().iter().any(|s| s.name.contains("update")), "{:?}", tm.spans());
    release.join().unwrap();
}
