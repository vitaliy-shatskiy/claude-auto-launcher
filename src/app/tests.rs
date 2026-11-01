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

#[test]
fn answers_resolve_by_full_key_or_longest_prefix() {
    let p = account_prefixes(&["work", "personal"]);
    assert_eq!(resolve_account_answer(" Personal ", &p, "work"), "personal");
    assert_eq!(resolve_account_answer("p", &p, "work"), "personal");
    assert_eq!(resolve_account_answer("", &p, "work"), "work");
    assert_eq!(resolve_account_answer("x", &p, "work"), "work");
}
