//! Project screen: the free-path row and its navigator.

use super::*;

#[test]
fn the_free_path_row_opens_the_navigator_and_picks_in_the_screen() {
    let f = fx();
    let path_row = ["s", "s", "s", "Enter"];
    let (miss, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        with_path(
            &path_row,
            r"C:\this-path-does-not-really-exist-9f3a\x",
            &["Enter", "Escape", "Escape"],
        ),
        "",
    );
    assert!(miss.is_none());
    assert_eq!(t.asked, 0, "the console line read is gone");
    let frames = t.inner.frames.unwrap();
    assert!(frames.iter().any(|fr| fr.join("\n").contains("path not found")));
    let scratch = f.tree.dir("free");
    let hit = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        with_path(&path_row, &format!("\"{scratch}\""), &["Enter"]),
        "",
    )
    .0
    .unwrap();
    assert_eq!((hit.path.as_str(), hit.slug.as_str()), (scratch.as_str(), ""));
    let resumed = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        with_path(&["RightArrow", "RightArrow", "s", "s", "s", "Enter"], &scratch, &["Enter"]),
        "",
    )
    .0
    .unwrap();
    assert_eq!((resumed.action.as_str(), resumed.path.as_str()), ("resume", scratch.as_str()));
    let d = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        with_path(&["s", "s", "s", "d", "Enter"], &scratch, &["Enter"]),
        "",
    )
    .0
    .unwrap();
    assert_eq!(d.action, "continue", "d steps the field on the free-path row too");
    let known = drive_with(&f.projects, &f.cwd, "new", with_path(&path_row, &f.beta, &["Enter"]), "")
        .0
        .unwrap();
    assert_eq!(known.slug, "B", "a typed known project carries its slugs");
}

/// Esc in the navigator returns to the list with no notice and never launches.
#[test]
fn escape_in_the_navigator_never_launches() {
    let f = fx();
    let (out, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        with_path(&["s", "s", "s", "Enter"], &f.alpha, &["Escape", "Escape"]),
        "",
    );
    assert!(out.is_none());
    let frames = t.inner.frames.unwrap();
    let last = frames.last().unwrap().join("\n");
    assert!(
        last.contains("known") && !last.contains("enter a path ") && !last.contains("not found"),
        "{last}"
    );
    assert!(last.contains("> + enter a path"), "the cursor is back on the free-path row\n{last}");
}

/// D1 in the navigator: one click selects a row, the second click on it acts.
#[test]
fn navigator_rows_take_a_click_then_a_second_click() {
    let f = fx();
    let root = format!(r"{}\root\", f.tree.0.display());
    let c = clock;
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c);
    s.nav = Some(Navigator::new(list_dirs, NETWORK_LIMIT));
    s.nav.as_mut().unwrap().text.clone_from(&root);
    s.before();
    let map = s.draw((80, 24)).map;
    let list = map.list.unwrap();
    let beta_y = list.first_row_y + 3; // use, .., alpha, beta
    let ev = with_path(&["s", "s", "s", "Enter"], &root, &[]);
    let mut once = ev.clone();
    once.extend([mouse(6, beta_y, true, false), key("Escape"), key("Escape")]);
    let (out, t) = drive_with(&f.projects, &f.cwd, "new", once, "");
    assert!(out.is_none());
    assert!(
        t.inner.frames.unwrap().iter().any(|fr| fr.join("\n").contains("> beta\\")),
        "the click selected beta"
    );
    let mut twice = ev;
    twice.extend([mouse(6, beta_y, true, false), mouse(6, beta_y, true, false), key("Enter")]);
    let picked = drive_with(&f.projects, &f.cwd, "new", twice, "").0.unwrap();
    assert_eq!(picked.path, f.beta, "the second click entered beta, Enter used it");
}

/// D5: the create question is answered only by Enter (or its footer button); a click or a cursor
/// move drops it, so a double-click never creates a folder.
#[test]
fn only_enter_answers_the_create_question() {
    let f = fx();
    let use_y = opening_nav_row0(&f);
    let path_row = ["s", "s", "s", "Enter"];
    let missing = |tag: &str| format!(r"{}\root\{tag}", f.tree.0.display());
    let click = || mouse(6, use_y, true, false);
    let cases: [(&str, Vec<Event>); 3] = [
        ("double", vec![key("Enter"), click(), click()]),
        ("triple", vec![click(), click(), click()]),
        ("moved", keys(&["Enter", "DownArrow", "UpArrow", "Enter"])),
    ];
    for (tag, gesture) in cases {
        let path = missing(tag);
        let mut ev = with_path(&path_row, &path, &[]);
        ev.extend(gesture);
        ev.extend(keys(&["Escape", "Escape", "Escape"]));
        let (out, t) = drive_with(&f.projects, &f.cwd, "new", ev, "");
        assert!(out.is_none(), "{tag}: launched");
        assert!(!Path::new(&path).exists(), "{tag}: created without a yes");
        let frames = t.inner.frames.unwrap();
        assert!(
            frames.iter().any(|fr| fr.join("\n").contains("yes, create it")),
            "{tag}: the question showed"
        );
    }
    let fresh = missing("yes");
    let yes = drive_with(&f.projects, &f.cwd, "new", with_path(&path_row, &fresh, &["Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!(yes.path, fresh, "Enter on the question creates");
}

/// Preview writes nothing: the create question's yes only says so.
#[test]
fn preview_never_creates_a_folder() {
    let f = fx();
    let fresh = format!(r"{}\root\preview-new", f.tree.0.display());
    let ev = with_path(&["s", "s", "s", "Enter"], &fresh, &["Enter", "Enter", "Escape", "Escape"]);
    let mut t = Script::new((80, 24), ev);
    let c = clock;
    let mut s = ProjectScreen::new(&f.projects, &f.cwd, "new", Style { color: false, ascii: true }, &c).with_preview(true);
    let out = crate::ui::screen_loop::run(&mut s, &mut t).unwrap();
    assert!(out.is_none(), "nothing picked");
    assert!(!Path::new(&fresh).exists(), "preview created the folder");
    let frames = t.inner.frames.unwrap();
    assert!(frames.iter().any(|fr| fr.join("\n").contains("preview - nothing was created")));
}

/// With no match, Enter on the free-path row still opens the navigator.
#[test]
fn no_match_still_opens_the_free_path_row() {
    let f = fx();
    let (out, t) = drive_with(
        &f.projects,
        &f.cwd,
        "new",
        keys(&["/", "z", "q", "DownArrow", "Enter", "Escape", "Escape", "Escape"]),
        "",
    );
    assert!(out.is_none());
    let frames = t.inner.frames.unwrap();
    assert!(
        frames.iter().any(|fr| fr.join("\n").contains("drop segment")),
        "the navigator opened"
    );
}
