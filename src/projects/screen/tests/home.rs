//! The launch page as a workspace over this screen: focus moves, Enter per panel, session
//! clicks, the narrow pages, and the shared click and hint walks.

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
fn a_click_on_a_session_selects_it_and_a_second_click_resumes_it() {
    let f = fx();
    let c = clock;
    let mut roster = crate::launch::tests::four();
    let v = view(false);
    let p = crate::prefs::Prefs::default();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let source = || Some(list(&f, v.style, &c));
    let st = roster.new_state();
    let mut s = LaunchScreen::new(&mut roster, st, &v, &p, 0, &mut hook).with_home(&source);
    // Given the list focused on alpha, its two sessions beside it
    let size = (160, 40);
    let _ = s.draw(size);
    let _ = s.tab();
    s.cursor().index = 1;
    s.before();
    let frame = s.draw(size);
    let ((x0, _), rows) = frame.map.side.expect("the sessions panel maps its rows");
    let second = rows.get(1).unwrap();
    // When the second session is clicked twice
    let (first_ended, out) = crate::ui::screen_loop::hover_walk::click_twice(&mut s, size, (x0 + 2, second.y));
    // Then the first click only selected it, the second resumed it.
    assert!(!first_ended, "a first click never acts");
    let out = out.expect("the second click resumes");
    assert_eq!(out.home_session, Some(("s2".to_owned(), false)));
}

#[test]
fn under_two_columns_the_panels_are_pages_in_the_same_order() {
    let f = fx();
    let (out, frames) = drive_home(&f, (100, 30), "Tab,s,Tab,Enter");
    // The launch page: the launch rows and the limits, no recent panel and no 1-5 keys.
    assert!(has(&frames[0], "* claude-auto") && !has(&frames[0], "recent"), "{:#?}", frames[0]);
    assert!(!has(&frames[0], "1-5"), "{:#?}", frames[0]);
    // Tab: the projects page alone.
    assert!(
        has(&frames[1], "* claude-auto - project") && !has(&frames[1], "limits"),
        "{:#?}",
        frames[1]
    );
    // Down onto alpha, Tab: the sessions page alone, its first session selected.
    assert!(
        has(&frames[3], "* claude-auto - sessions") && has(&frames[3], "title s1") && !has(&frames[3], "limits"),
        "{:#?}",
        frames[3]
    );
    // Every page fills the terminal to the footer row.
    for fr in &frames {
        assert_eq!(fr.len(), 29, "{fr:#?}");
    }
    assert_eq!(out.unwrap().home_session, Some(("s1".to_owned(), false)));
}

#[test]
fn the_filter_works_on_the_launch_page_list() {
    let f = fx();
    let (out, frames) = drive_home(&f, (160, 40), "Tab,/,b,e,t,Enter,Enter");
    assert!(has(&frames[5], "filter: bet"), "{:#?}", frames[5]);
    assert_eq!(out.unwrap().home_pick.unwrap().path, f.beta);
}

#[test]
fn a_focused_list_on_a_short_wide_page_is_drawn_and_its_keys_never_panic() {
    let f = fx();
    let (out, frames) = drive_home(&f, (211, 20), "Tab,Escape,Escape");
    assert!(
        has(&frames[1], "* claude-auto - project"),
        "the list is drawn, focused: {:#?}",
        frames[1]
    );
    assert!(out.is_none());
}

/// Whether the frame showed the focused panel's selected row: the list's rows on the list, the
/// cursor's session on the sessions panel, the launch rows on the launch panel.
fn row_shown(frame: &Frame, tabs: usize) -> bool {
    match tabs {
        0 => !frame.map.rows.is_empty(),
        1 => frame.map.list.as_ref().is_some_and(|l| l.row_count > 0),
        _ => frame.map.side.as_ref().is_some_and(|s| !s.1.is_empty()),
    }
}

/// Every size a terminal can report, every focus: no panic (debug arithmetic included), the
/// focused panel drawn whenever the terminal is not too small, and Enter on the list or the
/// sessions acts only on a row the frame showed.
#[test]
fn at_every_size_the_focused_panel_is_drawn_or_enter_does_nothing() {
    let f = fx();
    for size in crate::ui::screen_loop::hover_walk::every_size() {
        for tabs in 0..3 {
            focused_at(&f, size, tabs);
        }
    }
}

/// One size and focus of the sweep above (`tabs` Tab presses from the launch rows).
fn focused_at(f: &Fx, (w, h): (usize, usize), tabs: usize) {
    let c = clock;
    let v = view(false);
    let p = crate::prefs::Prefs::default();
    let source = || Some(list(f, v.style, &c));
    let mut roster = crate::launch::tests::four();
    let mut hook = |_: &KeyPress, _: &mut dyn Term| false;
    let st = roster.new_state();
    let mut s = LaunchScreen::new(&mut roster, st, &v, &p, 0, &mut hook).with_home(&source);
    if tabs > 0 {
        let _ = s.tab();
        // On alpha, whose two sessions the sessions panel lists.
        s.cursor().index = 1;
        s.before();
    }
    if tabs > 1 {
        let _ = s.tab();
    }
    let frame = s.draw((w, h));
    let shown = row_shown(&frame, tabs);
    if w >= crate::frame::MIN_WIDTH && h >= crate::frame::MIN_HEIGHT {
        assert!(shown, "{w}x{h} focus {tabs}: the focused panel is not drawn {:#?}", frame.lines);
    }
    if tabs > 0 && !shown {
        let acted = matches!(s.enter(), Some(Flow::Done(Some(_))));
        assert!(!acted, "{w}x{h} focus {tabs}: Enter acted on a row the frame did not show");
    }
}

thread_local! {
    /// How often the walk's launch hook opened maintenance.
    static OPENED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The click, stray-click and hint walks over the workspace on each focus.
#[test]
fn the_workspace_passes_the_click_and_hint_walks() {
    // One fixture tree, view and hiding for every screen of the walk.
    let f = fx();
    let c = clock;
    let color = Style { color: true, ascii: false };
    let v = view(true);
    let p = crate::prefs::Prefs::default();
    let save = |_: &str, _: HideChange| -> Result<(), String> {
        OPENED.with(|o| o.set(o.get() + 1));
        Ok(())
    };
    let hiding = Hiding {
        paths: RefCell::new(Vec::new()),
        pinned: RefCell::new(Vec::new()),
        save: &save,
    };
    let source = || Some(list(&f, color, &c).with_hiding(Some(&hiding)));
    for size in [(120, 30), (211, 52), (280, 72)] {
        for tabs in 0..3 {
            let mut make = || {
                // A walk step's hide or pin never carries into the next screen.
                hiding.paths.borrow_mut().clear();
                hiding.pinned.borrow_mut().clear();
                // A screen holds its rows and its key hook by `&mut`: each screen gets its own
                // (the hook captures nothing, so its box allocates nothing).
                let roster = Box::leak(Box::new(crate::launch::tests::four()));
                let hook: &mut crate::launch::screen::OnKey<'_> = Box::leak(Box::new(|k: &KeyPress, _: &mut dyn Term| {
                    let hit = crate::input::is_hotkey(k, 'u');
                    OPENED.with(|o| o.set(o.get() + usize::from(hit)));
                    hit
                }));
                let st = roster.new_state();
                let mut s = LaunchScreen::new(roster, st, &v, &p, 0, hook).with_home(&source);
                let _ = s.draw(size);
                for _ in 0..tabs {
                    let _ = s.tab();
                }
                s
            };
            // The click walks try every cell: 280x72 lays out as 211x52 does, at twice the cells.
            if size.0 < 280 {
                let mut s = make();
                assert_no_stray_click_acts(&mut s, size);
                let mut s = make();
                assert_first_click_never_acts(&mut s, size);
            }
            assert_every_hint_is_alive(&mut make, size, &|| OPENED.with(std::cell::Cell::get));
        }
    }
}
