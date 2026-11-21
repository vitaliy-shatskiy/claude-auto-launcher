//! The wide launch page as a workspace over this screen: focus moves, Enter per panel, the
//! narrow page that keeps the recent panel, and the shared click and hint walks.

use super::*;
use crate::input::vk;
use crate::launch::LaunchState;
use crate::launch::screen::{LaunchScreen, LaunchView};
use crate::projects::detail::SessionLine;
use crate::projects::project_key;
use crate::ui::screen_loop::hover_walk::{assert_every_hint_is_alive, assert_first_click_never_acts, assert_no_stray_click_acts};
use crate::ui::screen_loop::run;

fn view(color: bool) -> LaunchView {
    LaunchView {
        limits: crate::types::Limits::new(),
        version: crate::types::VersionInfo::default(),
        default_model_label: "default".into(),
        default_advisor_label: "default".into(),
        labels_by_account: std::collections::BTreeMap::new(),
        recent: vec![crate::launch::frame::RecentProject {
            name: "alpha".into(),
            ..Default::default()
        }],
        running: Vec::new(),
        mcp: String::new(),
        style: Style { color, ascii: !color },
    }
}

/// The list as the app hands it: the fixture's projects, the cwd row first, the alpha project's
/// sessions as its detail (in place of the loader).
fn list<'a>(f: &Fx, style: Style, clock: &'a dyn Fn() -> i64) -> ProjectScreen<'a> {
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", style, clock);
    s.detail = Some(Detail {
        key: project_key(&f.alpha),
        branch: None,
        count: 2,
        sessions: ["s1", "s2"]
            .iter()
            .map(|id| SessionLine {
                session_id: (*id).into(),
                title: format!("title {id}"),
                modified_ms: 5_000,
                live: false,
            })
            .collect(),
    });
    s
}

/// Runs the launch page over the fixture at `size` with `keys`; its end state and frames.
fn drive_home(f: &Fx, size: (usize, usize), keys: &str) -> (Option<LaunchState>, Vec<Vec<String>>) {
    drive_keys(f, size, crate::ui::parse_preview_keys(keys).unwrap())
}

fn drive_keys(f: &Fx, size: (usize, usize), keys: std::collections::VecDeque<KeyPress>) -> (Option<LaunchState>, Vec<Vec<String>>) {
    let c = clock;
    let mut roster = crate::launch::tests::four();
    let v = view(false);
    let p = crate::prefs::Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let source = || Some(list(f, v.style, &c));
    let st = roster.new_state();
    let mut s = LaunchScreen::new(&mut roster, st, &v, &p, 0, &mut hook).with_home(&source);
    let mut t = crate::ui::PreviewTerm::new(size, keys, true);
    let out = run(&mut s, &mut t).unwrap();
    (out, t.frames.unwrap())
}

fn has(frame: &[String], text: &str) -> bool {
    frame.iter().any(|l| l.contains(text))
}

#[test]
fn tab_moves_the_identity_from_the_launch_panel_to_the_list_and_on_to_the_sessions() {
    let f = fx();
    let (_, frames) = drive_home(&f, (160, 40), "Tab,s,Tab,Tab,Escape");
    let identity = |fr: &Vec<String>| fr.iter().position(|l| l.contains("* claude-auto")).unwrap();
    // Launch focused: the identity tops the page, the list under it is plain `projects`.
    assert_eq!(identity(&frames[0]), 0);
    assert!(has(&frames[0], "projects") && has(&frames[0], "running"), "{:#?}", frames[0]);
    // Tab: the list carries the identity, the right column the selected project's context.
    assert!(identity(&frames[1]) > 0 && has(&frames[1], "launch") && has(&frames[1], "selected"));
    // Down onto alpha, Tab: the sessions panel is focused, its first session selected.
    assert!(
        has(&frames[3], "* claude-auto - sessions") && has(&frames[3], "> "),
        "{:#?}",
        frames[3]
    );
    // Tab again wraps to the launch rows.
    assert_eq!(identity(&frames[4]), 0);
}

#[test]
fn shift_tab_moves_back_and_escape_steps_toward_the_launch_rows() {
    let f = fx();
    let mut keys = crate::ui::parse_preview_keys("Escape,Escape,Escape").unwrap();
    keys.push_front(KeyPress {
        shift: true,
        ..KeyPress::named(vk::TAB)
    });
    let (out, frames) = drive_keys(&f, (160, 40), keys);
    let identity = |fr: &Vec<String>| fr.iter().position(|l| l.contains("* claude-auto")).unwrap();
    assert!(
        has(&frames[1], "* claude-auto - sessions"),
        "Shift+Tab from the launch rows: the sessions"
    );
    assert!(
        identity(&frames[2]) > 0 && !has(&frames[2], "* claude-auto - sessions"),
        "Esc: the list"
    );
    assert_eq!(identity(&frames[3]), 0, "Esc again: the launch rows");
    assert!(out.is_none(), "Esc on the launch rows quits");
}

#[test]
fn enter_on_the_list_starts_a_new_session_there_with_the_launch_rows() {
    let f = fx();
    let (out, _) = drive_home(&f, (160, 40), "RightArrow,Tab,s,Enter");
    let out = out.unwrap();
    let pick = out.home_pick.unwrap();
    assert_eq!((pick.path.as_str(), pick.action.as_str()), (f.alpha.as_str(), "new"));
    assert_eq!(out.account, "personal", "the launch rows' choice rides along");
    assert!(out.home_session.is_none());
}

#[test]
fn enter_on_the_sessions_resumes_that_session_and_f_forks_it() {
    let f = fx();
    let (out, _) = drive_home(&f, (160, 40), "Tab,s,Tab,s,Enter");
    let out = out.unwrap();
    assert_eq!(out.home_session, Some(("s2".to_owned(), false)));
    assert_eq!(out.home_pick.unwrap().action, "resume");
    let (out, _) = drive_home(&f, (160, 40), "Tab,s,Tab,f");
    assert_eq!(out.unwrap().home_session, Some(("s1".to_owned(), true)));
}

#[test]
fn under_two_columns_the_page_keeps_the_recent_panel_and_enter_goes_on_to_the_project_page() {
    let f = fx();
    let (out, frames) = drive_home(&f, (100, 30), "Tab,Enter");
    assert!(has(&frames[0], "recent") && !has(&frames[0], "projects"), "{:#?}", frames[0]);
    let out = out.unwrap();
    assert!(out.home_pick.is_none(), "Enter is the launch rows' next: the project page follows");
}

#[test]
fn the_filter_works_on_the_launch_page_list() {
    let f = fx();
    let (out, frames) = drive_home(&f, (160, 40), "Tab,/,b,e,t,Enter,Enter");
    assert!(has(&frames[5], "filter: bet"), "{:#?}", frames[5]);
    assert_eq!(out.unwrap().home_pick.unwrap().path, f.beta);
}

thread_local! {
    /// How often the walk's launch hook opened maintenance.
    static OPENED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The click, stray-click and hint walks over the workspace on each focus.
#[test]
fn the_workspace_passes_the_click_and_hint_walks() {
    let f = fx();
    let c = clock;
    let color = Style { color: true, ascii: false };
    for size in [(120, 30), (211, 52), (280, 72)] {
        for tabs in 0..3 {
            let mut make = || {
                let roster = Box::leak(Box::new(crate::launch::tests::four()));
                let v: &'static LaunchView = Box::leak(Box::new(view(true)));
                let p: &'static crate::prefs::Prefs = Box::leak(Box::new(crate::prefs::Prefs::default()));
                let hook: &'static mut crate::launch::screen::OnKey<'static> = Box::leak(Box::new(|k: &KeyPress, _: &mut dyn Term| {
                    let hit = crate::input::is_hotkey(k, 'u');
                    OPENED.with(|o| o.set(o.get() + usize::from(hit)));
                    hit
                }));
                let fx: &'static Fx = Box::leak(Box::new(fx()));
                let cl: &'static dyn Fn() -> i64 = &clock;
                let save: &'static dyn Fn(&str, HideChange) -> Result<(), String> = &|_: &str, _: HideChange| {
                    OPENED.with(|o| o.set(o.get() + 1));
                    Ok(())
                };
                let hiding: &'static Hiding<'static> = Box::leak(Box::new(Hiding {
                    paths: RefCell::new(Vec::new()),
                    pinned: RefCell::new(Vec::new()),
                    save,
                }));
                let source: &'static dyn Fn() -> Option<ProjectScreen<'static>> =
                    Box::leak(Box::new(move || Some(list(fx, color, cl).with_hiding(Some(hiding)))));
                let st = roster.new_state();
                let mut s = LaunchScreen::new(roster, st, v, p, 0, hook).with_home(source);
                let _ = s.draw(size);
                for _ in 0..tabs {
                    let _ = s.tab();
                }
                s
            };
            let mut s = make();
            assert_no_stray_click_acts(&mut s, size);
            let mut s = make();
            assert_first_click_never_acts(&mut s, size);
            assert_every_hint_is_alive(&mut make, size, &|| OPENED.with(std::cell::Cell::get));
        }
    }
    let _ = (&f, c);
}
