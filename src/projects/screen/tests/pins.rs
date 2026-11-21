//! Project screen: `p`, `pinnedProjects`, and how a pin and a hide exclude each other.

use super::*;

fn config_in(f: &Fx) -> String {
    f.tree.0.join("claude-auto.json").to_string_lossy().into_owned()
}

fn on_disk(config: &str, key: &str) -> Value {
    serde_json::from_str::<Value>(&std::fs::read_to_string(config).unwrap()).unwrap()[key].clone()
}

/// The list line naming `name` (the title, which carries the notice, never counts).
fn line_of(frame: &[String], name: &str) -> Option<usize> {
    // Row 0 is the panel's top border: its title may name the project in a notice. Only the list
    // panel counts: the selected panel beside it names the project too.
    let list = |l: &String| l.split("||").next().unwrap_or_default().contains(name);
    frame.iter().skip(1).position(list).map(|i| i + 1)
}

#[test]
fn p_pins_a_project_on_top_saves_it_and_the_cursor_follows() {
    let f = fx();
    let config = config_in(&f);
    std::fs::write(&config, r#"{"remote": false}"#).unwrap();

    let frames = run_with_config(&f, &config, &["s", "s", "p", "Escape"]);
    let last = frames.last().unwrap();
    assert_eq!(on_disk(&config, "pinnedProjects"), serde_json::json!([f.beta]));
    assert!(on_disk(&config, "remote") == Value::Bool(false), "other keys kept");
    let (beta, alpha) = (line_of(last, "beta").unwrap(), line_of(last, "alpha").unwrap());
    assert!(beta < alpha, "the older beta is drawn on top: {last:#?}");
    assert!(
        last[beta].contains("> ^ beta"),
        "the cursor follows the pinned row, drawn with its pin mark: {last:#?}"
    );

    let restarted = run_with_config(&f, &config, &["s", "p", "Escape"]);
    assert!(
        line_of(&restarted[0], "beta").unwrap() < line_of(&restarted[0], "alpha").unwrap(),
        "the pin outlives the run"
    );
    assert_eq!(on_disk(&config, "pinnedProjects"), serde_json::json!([]), "p again unpins");
    let back = restarted.last().unwrap();
    assert!(
        line_of(back, "alpha").unwrap() < line_of(back, "beta").unwrap(),
        "recency order again"
    );
}

#[test]
fn pinning_a_hidden_project_unhides_it() {
    let f = fx();
    let config = config_in(&f);
    std::fs::write(&config, serde_json::json!({ "hiddenProjects": [f.alpha] }).to_string()).unwrap();

    let frames = run_with_config(&f, &config, &["v", "s", "p", "v", "Escape"]);
    assert_eq!(on_disk(&config, "hiddenProjects"), serde_json::json!([]));
    assert_eq!(on_disk(&config, "pinnedProjects"), serde_json::json!([f.alpha]));
    assert!(line_of(frames.last().unwrap(), "alpha").is_some(), "listed outside the hidden view");
}

#[test]
fn hiding_a_pinned_project_unpins_it() {
    let f = fx();
    let config = config_in(&f);
    std::fs::write(&config, serde_json::json!({ "pinnedProjects": [f.beta] }).to_string()).unwrap();

    let frames = run_with_config(&f, &config, &["s", "h", "Escape"]);
    assert_eq!(on_disk(&config, "pinnedProjects"), serde_json::json!([]));
    assert_eq!(on_disk(&config, "hiddenProjects"), serde_json::json!([f.beta]));
    let last = frames.last().unwrap();
    assert!(line_of(last, "beta").is_none(), "beta hidden: {last:#?}");
    assert!(last.iter().any(|l| l.contains("beta hidden and unpinned")), "{last:#?}");
}

#[test]
fn a_pinned_folder_that_is_gone_stays_in_config_and_is_listed_marked() {
    let f = fx();
    let config = config_in(&f);
    let gone = f.tree.0.join("gone").to_string_lossy().into_owned();
    let written = serde_json::json!({ "pinnedProjects": [gone] }).to_string();
    std::fs::write(&config, &written).unwrap();

    let frames = run_with_config(&f, &config, &["s", "Enter", "Escape"]);
    let first = &frames[0];
    let row = line_of(first, "gone (missing)").unwrap_or_else(|| panic!("listed: {first:#?}"));
    assert!(row < line_of(first, "alpha").unwrap(), "still on top");
    assert!(
        frames.last().unwrap().iter().any(|l| l.contains("path not found")),
        "Enter refuses it"
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), written, "the config keeps it");
}

#[test]
fn a_failed_pin_save_lasts_this_run_only_and_the_cwd_row_pins_nothing() {
    let f = fx();
    let save = |_: &str, _: HideChange| Err("disk full".to_owned());
    let hiding = Hiding {
        paths: RefCell::new(Vec::new()),
        pinned: RefCell::new(Vec::new()),
        save: &save,
    };
    let frames = run_hiding(&f, &hiding, keys(&["p", "s", "s", "p", "Escape"]));
    assert!(frames[1].iter().any(|l| l.contains("only a listed project")), "{:#?}", frames[1]);
    let last = frames.last().unwrap();
    assert!(last.iter().any(|l| l.contains("pinned for this run only: disk full")), "{last:#?}");
    assert_eq!(*hiding.pinned.borrow(), [f.beta.as_str()], "the pin holds for this run");
}

/// Mouse parity: a click selects the row, the footer's `p` acts on it.
#[test]
fn a_click_then_the_footer_p_pins_the_clicked_row() {
    let f = fx();
    let config = config_in(&f);
    let map = opening_map(&f);
    let ys = map.list.clone().unwrap().row_ys;
    let fy = map.footer_y.unwrap();
    let p = map.footer.iter().find(|s| s.ch == Some('p')).expect("a footer p button");
    let events = vec![
        mouse(4, ys[2], true, false),
        mouse(p.start, fy + i32::try_from(p.line).unwrap(), true, false),
        key("Escape"),
    ];
    run_events_with_config(&f, &config, events);
    assert_eq!(on_disk(&config, "pinnedProjects"), serde_json::json!([f.beta]));
}
