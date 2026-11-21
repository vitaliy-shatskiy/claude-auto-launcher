#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use super::*;
use crate::input::{Event, MouseEvent};
use crate::projects::registry::tests::Tree;
use crate::projects::select::tests::proj;
use crate::ui::UiStop;
use crate::ui::script_term::ScriptTerm as Script;
use std::path::Path;

/// Runs the screen until a pick or a cancel.
fn run_screen(
    term: &mut dyn Term,
    projects: &[Project],
    cwd: &Path,
    initial_action: &str,
    style: Style,
    clock: &dyn Fn() -> i64,
    hiding: Option<&Hiding<'_>>,
) -> Result<Option<ProjectChoice>, UiStop> {
    let cwd = cwd.to_string_lossy();
    let mut screen = ProjectScreen::new(projects, &cwd, initial_action, style, clock).with_hiding(hiding);
    crate::ui::screen_loop::run(&mut screen, term)
}

fn key(k: &str) -> Event {
    Event::Key(crate::input::scripted_key(k).unwrap())
}

fn keys(ks: &[&str]) -> Vec<Event> {
    ks.iter().map(|k| key(k)).collect()
}

fn mouse(x: i32, y: i32, left: bool, is_move: bool) -> Event {
    Event::Mouse(MouseEvent {
        x,
        y,
        left,
        buttons: u32::from(left),
        is_move,
        ..MouseEvent::default()
    })
}

struct Fx {
    tree: Tree,
    alpha: String,
    beta: String,
    cwd: String,
    projects: Vec<Project>,
}

fn fx() -> Fx {
    let t = Tree::new("screen");
    let alpha = t.dir(r"root\alpha");
    let beta = t.dir(r"root\beta");
    let cwd = t.dir("cwd");
    let projects = vec![
        Project {
            last_activity_ms: Some(2_000),
            ..proj("A", &alpha, "alpha")
        },
        Project {
            last_activity_ms: Some(1_000),
            ..proj("B", &beta, "beta")
        },
    ];
    Fx {
        tree: t,
        alpha,
        beta,
        cwd,
        projects,
    }
}

fn clock() -> i64 {
    10_000
}

fn drive_with(ps: &[Project], cwd: &str, initial: &str, events: Vec<Event>, answer: &str) -> (Option<ProjectChoice>, Script) {
    let mut t = Script::new((80, 24), events).answering(answer);
    let c = clock;
    let out = run_screen(&mut t, ps, Path::new(cwd), initial, Style { color: false, ascii: true }, &c, None).unwrap();
    (out, t)
}

fn drive(f: &Fx, ks: &[&str]) -> Option<ProjectChoice> {
    drive_with(&f.projects, &f.cwd, "new", keys(ks), "").0
}

/// The map of the frame the screen opens on.
fn opening_map(f: &Fx) -> crate::frame::RowMap {
    let c = clock;
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c);
    s.before();
    s.draw((80, 24)).map
}

/// One launcher run's project screen over `config`: `hiddenProjects` and `pinnedProjects` read
/// from the file, every change saved back to it as the launcher saves it.
fn run_with_config(f: &Fx, config: &str, ks: &[&str]) -> Vec<Vec<String>> {
    run_events_with_config(f, config, keys(ks))
}

fn run_events_with_config(f: &Fx, config: &str, events: Vec<Event>) -> Vec<Vec<String>> {
    let ctx = crate::config::PathCtx {
        home: f.tree.0.clone(),
        cwd: f.tree.0.clone(),
    };
    let cfg = crate::config::read_config(config, &ctx);
    let save = |path: &str, change: HideChange| crate::projects::save_hidden_project(config, &ctx, path, change).map_err(|e| e.to_string());
    let hiding = Hiding {
        paths: RefCell::new(cfg.hidden_projects),
        pinned: RefCell::new(cfg.pinned_projects),
        save: &save,
    };
    run_hiding(f, &hiding, events)
}

fn run_hiding(f: &Fx, hiding: &Hiding<'_>, events: Vec<Event>) -> Vec<Vec<String>> {
    let mut t = Script::new((80, 24), events);
    let c = clock;
    let style = Style { color: false, ascii: true };
    assert!(
        run_screen(&mut t, &f.projects, Path::new(&f.cwd), "new", style, &c, Some(hiding))
            .unwrap()
            .is_none()
    );
    t.inner.frames.unwrap()
}

/// `before` keys, then the path typed character by character, then `after` keys.
fn with_path(before: &[&str], path: &str, after: &[&str]) -> Vec<Event> {
    let mut ev = keys(before);
    ev.extend(path.chars().map(|c| Event::Key(KeyPress::char(c))));
    ev.extend(keys(after));
    ev
}

/// The y of the navigator's first row (the use row).
fn opening_nav_row0(f: &Fx) -> i32 {
    let c = clock;
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c);
    s.nav = Some(Navigator::new(list_dirs, NETWORK_LIMIT));
    s.nav.as_mut().unwrap().text = format!(r"{}\root\x", f.tree.0.display());
    s.before();
    s.draw((80, 24)).map.list.unwrap().first_row_y
}

/// The hover rule over the project screen at both layouts: list rows, the action values, buttons.
#[test]
fn every_clickable_region_of_the_project_screen_hovers_and_never_selects() {
    use crate::ui::screen_loop::hover_walk::assert_every_region_hovers;
    let f = fx();
    let c = clock;
    for size in [(130, 40), (90, 30)] {
        let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: true, ascii: false }, &c);
        assert_every_region_hovers(&mut s, size);
    }
    // The path navigator, in the same cockpit panel: its folder rows and the action values.
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: true, ascii: false }, &c);
    s.nav = Some(Navigator::new(list_dirs, NETWORK_LIMIT));
    s.nav.as_mut().unwrap().text = format!(r"{}\root\", f.tree.0.display());
    assert_every_region_hovers(&mut s, (130, 40));
}

/// In place (preview) the selected row's detail comes with the frame; a moved cursor asks again.
#[test]
fn the_context_follows_the_cursor() {
    let f = fx();
    let c = clock;
    let stores = crate::projects::detail::Stores {
        root: f.tree.0.join("no-projects-root"),
        live_dirs: Vec::new(),
    };
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c)
        .with_details(Some(DetailSource::in_place(stores)));
    s.before();
    let first = s.detail.clone().unwrap();
    assert_eq!(first.key, crate::projects::project_key(&f.cwd));
    s.cursor.index = 1;
    s.before();
    assert_eq!(s.detail.unwrap().key, crate::projects::project_key(&f.alpha));
}

mod hide;
mod keys;
mod mouse;
mod nav;
mod pins;
