use std::time::Duration;

use super::*;
use crate::log::Logger;

fn at(origin: Instant, ms: u64) -> Instant {
    origin + Duration::from_millis(ms)
}
#[test]
fn spans_nest_under_the_innermost_open_span_in_start_order() {
    let t = Timings::on(Instant::now());
    let outer = t.begin(&["a"]);
    let first = t.begin(&["b", "x"]);
    t.end(first);
    let second = t.begin(&["c"]);
    t.end(second);
    t.end(outer);
    let after = t.begin(&["d"]);
    t.end(after);
    let spans = t.spans();
    let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["a", "b:x", "c", "d"]);
    let parents: Vec<Option<usize>> = spans.iter().map(|s| s.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(0), None]);
    assert!(spans.iter().all(|s| s.dur_us.is_some()));
    assert!(spans.windows(2).all(|w| w[0].start_us <= w[1].start_us));
}

#[test]
fn a_scope_ends_its_span_on_drop_and_a_second_end_keeps_the_first() {
    let o = Instant::now();
    let t = Timings::on(o);
    {
        let _s = t.scope(&["config"]);
    }
    assert!(t.spans()[0].dur_us.is_some());
    let id = t.begin_at(&["x"], at(o, 10));
    t.end_at(id, at(o, 15));
    t.end_at(id, at(o, 99));
    assert_eq!(t.spans()[1].dur_us, Some(5000));
}

#[test]
fn the_off_recorder_records_nothing() {
    let t = Timings::default();
    let id = t.begin(&["a"]);
    t.end(id);
    assert!(!t.is_on());
    assert_eq!(t.spans(), Vec::new());
    assert_eq!(t.latest_ms("a"), None);
    assert_eq!(t.take_unlogged(), Vec::new());
}

#[test]
fn latest_ms_takes_the_last_ended_match_rounded_up() {
    let o = Instant::now();
    let t = Timings::on(o);
    let a = t.begin_at(&["key", "launch"], at(o, 0));
    t.end_at(a, at(o, 7));
    let b = t.begin_at(&["key", "picker"], at(o, 10));
    t.end_at(b, Instant::now().max(at(o, 10)) + Duration::from_micros(200));
    let short = t.begin_at(&["paint", "launch"], at(o, 1));
    t.end_at(short, at(o, 1) + Duration::from_micros(300));
    assert_eq!(t.latest_ms("paint:launch"), Some(1));
    assert_eq!(t.latest_ms("key:launch"), Some(7));
    assert!(t.latest_ms("key:picker").is_some_and(|v| v >= 1));
    assert_eq!(t.latest_ms("key:"), None, "names match exactly, never by prefix");
}

#[test]
fn unlogged_spans_are_handed_once_with_parent_names() {
    let o = Instant::now();
    let t = Timings::on(o);
    let root = t.begin_at(&["process"], at(o, 0));
    let c = t.begin_at(&["config"], at(o, 1));
    t.end_at(c, at(o, 3));
    let got = t.take_unlogged();
    assert_eq!(got, vec![rec("config", Some("process"), 2.0, 1.0)]);
    assert_eq!(t.take_unlogged(), Vec::new());
    t.end_at(root, at(o, 4));
    assert_eq!(t.take_unlogged(), vec![rec("process", None, 4.0, 0.0)]);
}

fn rec(name: &str, parent: Option<&str>, ms: f64, start_ms: f64) -> TimingRecord {
    TimingRecord {
        name: name.into(),
        parent: parent.map(str::to_owned),
        ms,
        start_ms,
        aborted: false,
    }
}

#[test]
fn end_key_drops_a_key_span_that_a_nested_screen_ran_inside() {
    let o = Instant::now();
    let t = Timings::on(o);
    let screen = t.begin_at(&["screen", "launch"], at(o, 0));
    let key = t.begin_at(&["key", "launch"], at(o, 1));
    let nested = t.begin_at(&["screen", "maintenance"], at(o, 2));
    t.end_at(nested, at(o, 900));
    t.end_key(key);
    t.end_at(screen, at(o, 1000));
    assert_eq!(t.latest_ms("key:launch"), None);
    let recs = t.take_unlogged();
    assert!(recs.iter().all(|r| r.name != "key:launch"), "{recs:?}");
    let m = recs.iter().find(|r| r.name == "screen:maintenance").unwrap();
    assert_eq!(m.parent.as_deref(), Some("screen:launch"));
    let plain = t.begin_at(&["key", "launch"], at(o, 1001));
    t.end_key(plain);
    assert!(t.latest_ms("key:launch").is_some());
}

#[test]
fn ending_a_span_closes_its_open_children_as_aborted_and_they_adopt_nothing() {
    let o = Instant::now();
    let t = Timings::on(o);
    let ui = t.begin_at(&["ui"], at(o, 0));
    let load = t.begin_at(&["load", "launch"], at(o, 1));
    t.begin_at(&["console"], at(o, 2));
    // An early return skips both ends; the enclosing span's end closes them.
    t.end_at(ui, at(o, 5));
    let later = t.begin_at(&["prepare"], at(o, 6));
    t.end_at(later, at(o, 7));
    t.end_at(load, at(o, 9));
    let s = t.spans();
    assert_eq!(s[3].parent, None, "a later span is not adopted by an abandoned one");
    assert_eq!((s[1].dur_us, s[1].aborted), (Some(4000), true));
    assert_eq!((s[2].dur_us, s[2].aborted), (Some(3000), true));
    assert!(!s[0].aborted);
    let logged = t.take_unlogged();
    assert_eq!(logged.iter().filter(|r| r.aborted).count(), 2);
    assert_eq!(logged.len(), 4);
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("claude-auto-rs-timing-{name}-{}", std::process::id()));
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn debug_is_the_lone_flag_or_the_variable() {
    let a = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert!(debug_requested(&a(&["--launcher-debug"]), None));
    assert!(is_debug_flag(&a(&["--launcher-debug"])));
    assert!(debug_requested(&[], Some("1")));
    assert!(!debug_requested(&a(&["--launcher-debug", "-p", "x"]), None));
    assert!(!debug_requested(&a(&["--debug"]), None));
    assert!(!debug_requested(&[], Some("0")));
    // The variable turns debug on but never takes an argument away.
    assert!(debug_requested(&a(&["--debug"]), Some("1")) && !is_debug_flag(&a(&["--debug"])));
}

#[test]
fn debug_off_writes_no_timing_records() {
    let d = scratch("off");
    let l = Logger {
        root: d.join("logs"),
        run_id: "r".into(),
        enabled: true,
        timings: Timings::default(),
    };
    let id = l.timings.begin(&["config"]);
    l.timings.end(id);
    l.write_timings();
    assert!(!l.root.exists());
    let on = Logger {
        timings: Timings::on(Instant::now()),
        ..l
    };
    let id = on.timings.begin(&["config"]);
    on.timings.end(id);
    on.write_timings();
    let file = std::fs::read_dir(&on.root).unwrap().next().unwrap().unwrap().path();
    let text = std::fs::read_to_string(file).unwrap();
    assert_eq!(text.lines().filter_map(summary::record).count(), 1);
    std::fs::remove_dir_all(&d).unwrap();
}
