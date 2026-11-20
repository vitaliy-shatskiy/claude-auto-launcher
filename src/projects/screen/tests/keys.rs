//! Project screen: Enter, the hotkeys, Escape, the filter box and the action field.

use super::*;

#[test]
fn enter_hotkeys_and_escape() {
    let f = fx();
    let p1 = drive(&f, &["s", "Enter"]).unwrap();
    assert_eq!(
        (p1.path.as_str(), p1.action.as_str(), p1.slug.as_str(), p1.how.as_str()),
        (f.alpha.as_str(), "new", "A", "enter")
    );
    let p2 = drive(&f, &["s", "s", "c"]).unwrap();
    assert_eq!(
        (p2.path.as_str(), p2.action.as_str(), p2.slug.as_str()),
        (f.beta.as_str(), "continue", "B")
    );
    assert_eq!(drive(&f, &["r"]).unwrap().action, "resume");
    assert_eq!(drive(&f, &["t"]).unwrap().action, "worktree");
    assert!(drive(&f, &["Escape"]).is_none());
    let p5 = drive(&f, &["Enter"]).unwrap();
    assert_eq!((p5.path.as_str(), p5.slug.as_str()), (f.cwd.as_str(), ""));
    let variant = format!("{}/", f.alpha.replace('\\', "/").to_uppercase());
    assert_eq!(drive_with(&f.projects, &variant, "new", keys(&["Enter"]), "").0.unwrap().slug, "A");
}

#[test]
fn the_filter_owns_the_keys_and_parks_on_its_match() {
    let f = fx();
    assert_eq!(drive(&f, &["/", "e", "t", "a", "Enter", "Enter"]).unwrap().path, f.beta);
    assert!(drive(&f, &["/", "c", "Escape", "Escape"]).is_none(), "c edits the filter");
    assert_eq!(
        drive(&f, &["/", "z", "Escape", "Enter"]).unwrap().path,
        f.cwd,
        "first Escape clears"
    );
    assert!(drive(&f, &["/", "z", "Escape", "Escape"]).is_none());
    let mut chars: Vec<String> = vec!["/".into()];
    chars.extend(f.alpha.chars().map(String::from));
    chars.extend(["Enter".into(), "Enter".into()]);
    let ks: Vec<&str> = chars.iter().map(String::as_str).collect();
    assert_eq!(drive(&f, &ks).unwrap().path, f.alpha, "a full path as the filter");
}

#[test]
fn the_filter_keeps_short_name_and_bracket_chars() {
    let t = Tree::new("screen-odd");
    let odd = t.dir(r"RUNNER~1\app (x86)+c@home");
    let cwd = t.dir("cwd");
    let projects = vec![proj("O", &odd, "odd")];
    let mut ks: Vec<String> = vec!["/".into()];
    ks.extend(odd.chars().map(String::from));
    ks.extend(["Enter".into(), "Enter".into()]);
    let refs: Vec<&str> = ks.iter().map(String::as_str).collect();
    let picked = drive_with(&projects, &cwd, "new", keys(&refs), "").0;
    assert_eq!(picked.map(|p| p.path), Some(odd), "a path with ~, brackets, + and @ as the filter");
}

#[test]
fn a_cwd_that_is_a_project_runs_there_and_parks_first() {
    let f = fx();
    let p = drive_with(&f.projects, &f.beta, "new", keys(&["Enter"]), "").0.unwrap();
    assert_eq!((p.path.as_str(), p.slug.as_str()), (f.beta.as_str(), "B"));
    assert_eq!(
        drive_with(&f.projects, &f.beta, "new", keys(&["s", "Enter"]), "").0.unwrap().path,
        f.alpha
    );
    let betamax = f.tree.dir(r"root\betamax");
    let mut ps = f.projects.clone();
    ps.push(proj("BM", &betamax, "betamax"));
    let parked = drive_with(&ps, &f.beta, "new", keys(&["/", "e", "t", "a", "Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!(parked.path, f.beta, "the cwd row's match wins the park");
    let other = drive_with(&f.projects, &f.beta, "new", keys(&["/", "a", "l", "p", "Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!(other.path, f.alpha);
}

#[test]
fn a_vanished_row_is_refused_and_the_hotkey_still_sets_the_field() {
    let f = fx();
    let gone = f.tree.0.join("vanished").to_string_lossy().into_owned();
    let ps = vec![proj("V", &gone, "vanished")];
    assert!(drive_with(&ps, &f.cwd, "new", keys(&["s", "Enter", "Escape"]), "").0.is_none());
    for (k, want) in [("t", "worktree"), ("c", "continue"), ("r", "resume")] {
        let (out, t) = drive_with(&ps, &gone, "new", keys(&[k, "Escape"]), "");
        assert!(out.is_none());
        let frames = t.inner.frames.unwrap();
        let second = frames[1].join("\n");
        assert!(second.contains(&format!("[{want}]")), "{k}: the field the press left behind");
        assert!(second.contains("path not found"), "{k}: the notice");
    }
}

#[test]
fn the_action_field_steps_from_any_row_and_wraps() {
    let f = fx();
    let a1 = drive(&f, &["s", "RightArrow", "Enter"]).unwrap();
    assert_eq!((a1.action.as_str(), a1.path.as_str()), ("continue", f.alpha.as_str()));
    assert_eq!(drive(&f, &["RightArrow", "RightArrow", "Enter"]).unwrap().action, "resume");
    assert_eq!(
        drive(&f, &["RightArrow", "RightArrow", "RightArrow", "RightArrow", "Enter"])
            .unwrap()
            .action,
        "new"
    );
    assert_eq!(drive(&f, &["LeftArrow", "Enter"]).unwrap().action, "worktree");
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "resume", keys(&["Enter"]), "").0.unwrap().action,
        "resume"
    );
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "rm -rf", keys(&["Enter"]), "").0.unwrap().action,
        "new"
    );
    assert_eq!(
        drive_with(&f.projects, &f.cwd, "RESUME", keys(&["RightArrow", "Enter"]), "")
            .0
            .unwrap()
            .action,
        "worktree"
    );
    let d = drive(&f, &["s", "d", "Enter"]).unwrap();
    assert_eq!((d.action.as_str(), d.path.as_str()), ("continue", f.alpha.as_str()));
    assert_eq!(drive(&f, &["s", "a", "Enter"]).unwrap().action, "worktree");
}

#[test]
fn down_stops_at_the_free_path_row() {
    let f = fx();
    let (out, t) = drive_with(&f.projects, &f.cwd, "new", keys(&["s", "s", "s", "s", "s", "w", "Escape"]), "");
    assert!(out.is_none());
    let frames = t.inner.frames.unwrap();
    assert_eq!(frames.len(), 7);
    let cursor_line = |i: usize| frames[i].iter().position(|l| l.contains(" > ")).unwrap();
    assert_eq!(cursor_line(3), cursor_line(4));
    assert_eq!(cursor_line(4), cursor_line(5));
    assert!(cursor_line(6) < cursor_line(5));
}

/// D4: Enter in the open filter runs the highlighted row; the arrows move while typing.
#[test]
fn enter_and_arrows_work_inside_the_filter() {
    let f = fx();
    assert_eq!(drive(&f, &["/", "a", "l", "p", "Enter"]).unwrap().path, f.alpha);
    assert_eq!(
        drive(&f, &["/", "r", "o", "o", "t", "DownArrow", "Enter"]).unwrap().path,
        f.beta,
        "the down arrow moved, it was not typed"
    );
    let (out, t) = drive_with(&f.projects, &f.cwd, "new", keys(&["/", "z", "q", "Enter", "Escape", "Escape"]), "");
    assert!(out.is_none(), "Enter on no match never runs the current directory");
    assert!(t.inner.frames.unwrap()[4].join("\n").contains("no project matches"));
}

/// Item 3: Esc clears a kept filter, leaves the hidden view, and only then ends the screen.
#[test]
fn escape_steps_back_one_level() {
    let f = fx();
    let gone = f.tree.0.join("vanished").to_string_lossy().into_owned();
    let mut ps = f.projects.clone();
    ps.insert(0, proj("V", &gone, "vanished"));
    // Enter on a vanished row is refused: the filter closes and stays.
    let (out, t) = drive_with(&ps, &f.cwd, "new", keys(&["/", "v", "a", "n", "Enter", "Escape", "Escape"]), "");
    assert!(out.is_none());
    let frames = t.inner.frames.unwrap();
    let n = frames.len();
    assert!(frames[n - 2].join("\n").contains("filter: van"), "the kept filter");
    assert!(
        !frames[n - 1].join("\n").contains("filter:"),
        "the first Esc cleared it, the screen stayed"
    );
    let (out, t) = drive_with(&f.projects, &f.cwd, "new", keys(&["v", "Escape", "Escape"]), "");
    assert!(out.is_none());
    assert_eq!(t.inner.frames.unwrap().len(), 3, "v, then Esc back to the list, then Esc out");
}
