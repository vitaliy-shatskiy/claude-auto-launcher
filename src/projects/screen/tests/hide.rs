//! Project screen: `h`, `hiddenProjects` and the hidden rows.

use super::*;

#[test]
fn h_hides_and_unhides_in_show_hidden() {
    let f = fx();
    let config = f.tree.0.join("claude-auto.json");
    let config = config.to_str().unwrap();
    std::fs::write(config, r#"{"remote": false}"#).unwrap();
    // The title line ("N known") carries the notice naming the project: only list rows count.
    let listed = |frame: &[String]| frame.iter().any(|l| l.contains("alpha") && !l.contains("known"));

    let first = run_with_config(&f, config, &["s", "h", "Escape"]);
    assert!(!listed(first.last().unwrap()), "h hides alpha at once");

    let restarted = run_with_config(&f, config, &["Escape"]);
    assert!(!listed(&restarted[0]), "the launcher restarts: alpha stays hidden");
    assert!(
        std::fs::read_to_string(config).unwrap().contains("\"remote\": false"),
        "other keys kept"
    );

    let shown = run_with_config(&f, config, &["v", "s", "h", "v", "Escape"]);
    assert!(
        shown[1].iter().any(|l| l.contains("alpha (hidden)")),
        "the show-hidden view lists it marked: {:#?}",
        shown[1]
    );
    assert!(listed(shown.last().unwrap()), "h there un-hides it");
    let again = run_with_config(&f, config, &["Escape"]);
    assert!(listed(&again[0]), "and it stays listed");
}

#[test]
fn h_changes_only_its_own_entry_and_keeps_the_rest_as_written() {
    let f = fx();
    let config = f.tree.0.join("claude-auto.json");
    let config = config.to_str().unwrap();
    let kept = serde_json::json!([r"~\keep", r"%APPDATA%\x", "relative"]);
    let mut written = kept.as_array().unwrap().clone();
    written.insert(1, serde_json::json!(f.alpha.replace('\\', "/").to_uppercase()));
    std::fs::write(config, serde_json::json!({ "hiddenProjects": written }).to_string()).unwrap();
    let on_disk =
        || -> Value { serde_json::from_str::<Value>(&std::fs::read_to_string(config).unwrap()).unwrap()["hiddenProjects"].clone() };

    run_with_config(&f, config, &["v", "s", "h", "Escape", "Escape"]);
    assert_eq!(on_disk(), kept, "unhide removes the entry whose expansion matches, nothing else");

    run_with_config(&f, config, &["s", "h", "Escape"]);
    let mut appended = kept.as_array().unwrap().clone();
    appended.push(serde_json::json!(f.alpha));
    assert_eq!(on_disk(), Value::Array(appended), "hide appends the path, the rest as written");
}

#[test]
fn unhide_shows_a_project_listed_under_two_spellings_at_once() {
    let f = fx();
    let config = f.tree.0.join("claude-auto.json");
    let config = config.to_str().unwrap();
    let twice = serde_json::json!([f.alpha, f.alpha.replace('\\', "/").to_uppercase()]);
    std::fs::write(config, serde_json::json!({ "hiddenProjects": twice }).to_string()).unwrap();
    let listed = |frame: &[String]| frame.iter().any(|l| l.contains("alpha") && !l.contains("known"));

    let frames = run_with_config(&f, config, &["v", "s", "h", "v", "Escape"]);
    assert!(listed(frames.last().unwrap()), "both spellings go in this run, not only the first");
}

#[test]
fn a_failed_save_says_the_hide_lasts_this_run_only() {
    let f = fx();
    let save = |_: &str, _: HideChange| Err("disk full".to_owned());
    let hiding = Hiding {
        paths: RefCell::new(Vec::new()),
        save: &save,
    };
    let frames = run_hiding(&f, &hiding, keys(&["s", "h", "Escape"]));
    let last = frames.last().unwrap();
    assert!(last.iter().any(|l| l.contains("hidden for this run only: disk full")), "{last:#?}");
    assert_eq!(*hiding.paths.borrow(), [f.alpha.as_str()], "the hide holds for this run");
}

#[test]
fn only_an_exact_h_hides_a_shifted_one_never_writes() {
    let f = fx();
    let config = f.tree.0.join("claude-auto.json");
    let config = config.to_str().unwrap();
    // A split X10 mouse report arrives as Shift + an upper-case letter.
    let shifted = Event::Key(KeyPress {
        shift: true,
        ..KeyPress::char('H')
    });
    run_events_with_config(&f, config, vec![key("s"), shifted, key("Escape")]);
    assert!(!Path::new(config).exists(), "nothing saved");
    let caps = Event::Key(KeyPress {
        caps_lock: true,
        ..KeyPress::char('H')
    });
    run_events_with_config(&f, config, vec![key("s"), caps, key("Escape")]);
    assert!(
        std::fs::read_to_string(config).unwrap().contains("alpha"),
        "Caps Lock h still hides"
    );
}

#[test]
fn h_on_the_current_directory_hides_nothing() {
    let f = fx();
    let config = f.tree.0.join("c.json");
    let config = config.to_str().unwrap();
    let frames = run_with_config(&f, config, &["h", "Escape"]);
    assert!(frames.last().unwrap().iter().any(|l| l.contains("only a listed project")));
    assert!(!Path::new(config).exists(), "nothing saved");
}

#[test]
fn hidden_rows_are_reached_through_the_filter() {
    let f = fx();
    let probe = f.tree.dir(r"temp\probe-1");
    let job = f.tree.dir(r"p\jobs\1\tmp\w585");
    let mut ps = f.projects.clone();
    ps.push(Project {
        hidden: true,
        ..proj("T", &probe, "probe-1")
    });
    ps.push(Project {
        hidden: true,
        ..proj("J", &job, "w585")
    });
    let picked = drive_with(&ps, &f.cwd, "new", keys(&["/", "w", "5", "8", "Enter", "Enter"]), "")
        .0
        .unwrap();
    assert_eq!((picked.path.as_str(), picked.slug.as_str()), (job.as_str(), "J"));
    let stepped = drive_with(&ps, &f.cwd, "new", with_path(&["s", "s", "s", "Enter"], &probe, &["Enter"]), "")
        .0
        .unwrap();
    assert_eq!(stepped.path, probe, "the unfiltered list does not show the hidden rows");
}
