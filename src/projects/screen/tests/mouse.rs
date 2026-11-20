//! Project screen: clicks, hover, wheel and the footer buttons.

use super::*;
use crate::input::vk;

#[test]
fn mouse_click_moves_second_click_commits_and_values_walk() {
    let f = fx();
    let map = opening_map(&f);
    let ys = map.list.clone().unwrap().row_ys;
    let (one, _) = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, ys[1], true, false), key("Escape")], "");
    assert!(one.is_none(), "one click starts nothing");
    let moved = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, ys[1], true, false), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(moved.path, f.alpha);
    let twice = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![mouse(4, ys[1], true, false), mouse(4, ys[1], true, false), key("Escape")],
        "",
    )
    .0
    .unwrap();
    assert_eq!((twice.path.as_str(), twice.how.as_str()), (f.alpha.as_str(), "mouse"));
    let after_gap = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, ys[2], true, false), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(after_gap.path, f.beta, "the gap is not hit-testable");
    let a = map.action.unwrap();
    let pick = |x: i32| {
        drive_with(&f.projects, &f.cwd, "new", vec![mouse(x, a.y, true, false), key("Enter")], "")
            .0
            .unwrap()
    };
    let cont = pick(a.cells[1].start);
    assert_eq!((cont.action.as_str(), cont.path.as_str()), ("continue", f.cwd.as_str()));
    assert_eq!(pick(a.cells[3].end).action, "worktree");
    assert_eq!(pick(1).action, "new", "between the cells changes nothing");
    let detail = drive_with(&f.projects, &f.cwd, "new", vec![mouse(4, a.y - 1, true, false), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(detail.path, f.cwd, "the detail line is inert");
    let committed = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![
            key("RightArrow"),
            mouse(4, ys[1], true, false),
            mouse(4, ys[1], true, false),
            key("Escape"),
        ],
        "",
    )
    .0
    .unwrap();
    assert_eq!((committed.action.as_str(), committed.path.as_str()), ("continue", f.alpha.as_str()));
}

/// Item 4: a click on a footer button of the open filter is a key, never typed into the filter.
#[test]
fn a_footer_click_in_the_open_filter_is_never_typed() {
    let f = fx();
    let c = clock;
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c);
    s.cursor.typing = true;
    s.before();
    let map = s.draw((80, 24)).map;
    let fy = map.footer_y.unwrap();
    let esc = map.footer.iter().find(|s| s.key == "Escape").unwrap();
    let (out, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![
            key("/"),
            key("a"),
            mouse(esc.start, fy + i32::try_from(esc.line).unwrap(), true, false),
            key("Escape"),
        ],
        "",
    );
    assert!(out.is_none());
    let last = t.inner.frames.unwrap().last().unwrap().join("\n");
    assert!(!last.contains("filter:"), "the click was Esc: the filter is cleared\n{last}");
}

/// D1: the keyboard puts the cursor on alpha; one click there selects, it never starts a session.
#[test]
fn one_click_on_a_keyboard_cursor_row_starts_nothing() {
    let f = fx();
    let ys = opening_map(&f).list.unwrap().row_ys;
    let (out, _) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![key("s"), mouse(4, ys[1], true, false), key("Escape")],
        "",
    );
    assert!(out.is_none(), "a click on the keyboard cursor launched");
    let picked = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![key("s"), mouse(4, ys[1], true, false), key("Enter")],
        "",
    )
    .0
    .unwrap();
    assert_eq!(picked.path, f.alpha, "keyboard Enter still runs the cursor row");
}

#[test]
fn hover_paints_never_selects_and_a_notice_outlives_the_mouse() {
    let f = fx();
    let map = opening_map(&f);
    let ys = map.list.clone().unwrap().row_ys;
    let p = drive_with(&f.projects, &f.cwd, "new", vec![mouse(3, ys[2], false, true), key("Enter")], "")
        .0
        .unwrap();
    assert_eq!(p.path, f.cwd, "hover leaves the cursor");
    let (_, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![mouse(3, ys[0], false, true), mouse(4, ys[0], false, true), key("Escape")],
        "",
    );
    assert_eq!(t.inner.frames.unwrap().len(), 2, "a second identical move draws nothing");
    let a = map.action.unwrap();
    let (_, t2) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        vec![mouse(3, a.y, false, true), mouse(4, a.y, false, true), key("Escape")],
        "",
    );
    assert_eq!(
        t2.inner.frames.unwrap().len(),
        1,
        "the action row outside a value is no hover change"
    );
    let fy = map.footer_y.unwrap();
    let c = map.footer.iter().find(|s| s.ch == Some('c')).unwrap();
    let events = vec![
        key("s"),
        key("Enter"),
        mouse(c.start, fy + i32::try_from(c.line).unwrap(), false, true),
        key("w"),
        key("Escape"),
    ];
    let gone = f.tree.0.join("vanished").to_string_lossy().into_owned();
    let (_, t3) = drive_with(&[proj("V", &gone, "vanished")], &f.cwd, "new", events, "");
    let frames: Vec<String> = t3.inner.frames.unwrap().iter().map(|l| l.join("\n")).collect();
    let n = frames.len();
    assert!(
        frames[n - 3].contains("path not found") && frames[n - 2].contains("path not found"),
        "a move keeps it"
    );
    assert!(!frames[n - 1].contains("path not found"), "the next key clears it");
}

#[test]
fn wheel_ctrl_c_and_an_uppercase_hotkey() {
    let f = fx();
    let down = Event::Mouse(MouseEvent {
        wheel: -128,
        ..MouseEvent::default()
    });
    let up = Event::Mouse(MouseEvent {
        wheel: 128,
        ..MouseEvent::default()
    });
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "new", vec![down, key("Enter")], "").0.unwrap().path,
        f.alpha
    );
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "new", vec![down, up, key("Enter")], "")
            .0
            .unwrap()
            .path,
        f.cwd
    );
    let ctrl_c = KeyPress {
        vk: vk::C,
        ctrl: true,
        ch: '\u{3}',
        ..KeyPress::default()
    };
    assert!(drive_with(&f.projects, &f.cwd, "new", vec![Event::Key(ctrl_c)], "").0.is_none());
    let upper_c = KeyPress {
        vk: vk::C,
        ch: 'C',
        ..KeyPress::default()
    };
    let picked = drive_with(&f.projects, &f.cwd, "new", vec![Event::Key(upper_c), key("Escape")], "").0;
    assert_eq!(
        picked.map(|p| p.action),
        Some("continue".to_owned()),
        "Caps Lock's 'C' (psmux drops the flag) is still the continue hotkey"
    );
}
