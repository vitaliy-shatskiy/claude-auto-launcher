use std::time::Duration;

use super::summary::{self, Row};
use super::view::{footer_text, tree_lines, with_footer};
use super::*;
use crate::layout::display_width;
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
    assert!(t.latest_ms("key:").is_some_and(|v| v >= 1));
    assert_eq!(t.latest_ms("load:"), None);
}

#[test]
fn unlogged_spans_are_handed_once_with_parent_names() {
    let o = Instant::now();
    let t = Timings::on(o);
    let root = t.begin_at(&["process"], at(o, 0));
    let c = t.begin_at(&["config"], at(o, 1));
    t.end_at(c, at(o, 3));
    let got = t.take_unlogged();
    assert_eq!(got, vec![("config".to_owned(), Some("process".to_owned()), 2.0, 1.0)]);
    assert_eq!(t.take_unlogged(), Vec::new());
    t.end_at(root, at(o, 4));
    assert_eq!(t.take_unlogged(), vec![("process".to_owned(), None, 4.0, 0.0)]);
}

#[test]
fn tree_lists_children_under_their_parent_slowest_first() {
    let o = Instant::now();
    let t = Timings::on(o);
    let top = t.begin_at(&["a"], at(o, 0));
    let fast = t.begin_at(&["b"], at(o, 0));
    t.end_at(fast, at(o, 2));
    let slow = t.begin_at(&["c"], at(o, 2));
    let inner = t.begin_at(&["c1"], at(o, 2));
    t.end_at(inner, at(o, 3));
    t.end_at(slow, at(o, 7));
    t.end_at(top, at(o, 10));
    t.begin_at(&["z"], at(o, 10));
    assert_eq!(
        tree_lines(&t.spans(), 30_000),
        [
            "z  20.00 ms (open)",
            "a  10.00 ms",
            "  c  5.00 ms",
            "    c1  1.00 ms",
            "  b  2.00 ms"
        ]
    );
}

#[test]
fn footer_full_words_in_the_two_pane_tier() {
    assert_eq!(
        footer_text(Some(12), Some(3), Some(4), 100, false),
        "  load 12 ms · paint 3 ms · last key->frame 4 ms"
    );
    assert_eq!(
        footer_text(None, Some(3), None, 100, true),
        "  load - ms | paint 3 ms | last key->frame - ms"
    );
}

#[test]
fn footer_at_the_minimum_width_keeps_full_words_while_they_fit() {
    let full = footer_text(Some(12), Some(3), Some(4), 50, false);
    assert_eq!(full, "  load 12 ms · paint 3 ms · last key->frame 4 ms");
    assert_eq!(
        footer_text(Some(1234), Some(567), Some(89), 50, false),
        "  load 1234 · paint 567 · key 89 ms"
    );
}

#[test]
fn footer_below_the_minimum_width_falls_back_to_numbers_and_never_overflows() {
    assert_eq!(footer_text(Some(1234), Some(567), Some(89), 30, false), "1234/567/89 ms");
    for w in 1..=120 {
        let t = footer_text(Some(1234), Some(567), Some(89), w, false);
        assert!(display_width(&t) <= w.saturating_sub(1), "width {w}: {t}");
    }
}

#[test]
fn with_footer_owns_the_last_row_only_when_on() {
    let lines: Vec<String> = (0..30).map(|i| format!("row {i}")).collect();
    assert_eq!(with_footer(&Timings::default(), lines.clone(), "launch", (80, 24)), lines);
    let t = Timings::on(Instant::now());
    let out = with_footer(&t, lines, "launch", (80, 24));
    assert_eq!(out.len(), 24);
    assert_eq!(out[22], "row 22");
    assert!(out[23].starts_with("  load - ms"));
    let short = with_footer(&t, vec!["one".into()], "launch", (80, 5));
    assert_eq!(short.len(), 5);
    assert_eq!(short[1], "");
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
fn summary_math_over_a_fixture_log() {
    let d = scratch("fixture");
    let mut text = String::from("{\"stage\":\"start\",\"ms\":999}\r\nnot json\r\n");
    text.extend(
        [7, 3, 20, 1, 15, 9, 2, 18, 11, 5, 14, 4, 19, 6, 13, 10, 8, 17, 12, 16]
            .map(|v| format!("{{\"stage\":\"timing\",\"name\":\"paint:launch\",\"ms\":{v}}}\r\n")),
    );
    text.push_str("{\"stage\":\"timing\",\"name\":\"config\",\"ms\":3.5}\r\n");
    let f = d.join("claude-auto-2026-11-01.jsonl");
    std::fs::write(&f, text).unwrap();
    let rows = summary::summarize(
        &std::fs::read_to_string(&f)
            .unwrap()
            .lines()
            .filter_map(summary::record)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        rows,
        [
            Row {
                name: "config".into(),
                count: 1,
                p50: 3.5,
                p95: 3.5,
                max: 3.5
            },
            Row {
                name: "paint:launch".into(),
                count: 20,
                p50: 10.0,
                p95: 19.0,
                max: 20.0
            },
        ]
    );
    let table = summary::report(&[f]);
    assert_eq!(table.len(), 3);
    assert!(table[2].starts_with("paint:launch") && table[2].ends_with("20.00"));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn percentile_is_nearest_rank() {
    let v = [1.0, 2.0, 3.0, 4.0];
    assert_eq!(summary::percentile(&v, 50), 2.0);
    assert_eq!(summary::percentile(&v, 95), 4.0);
    assert_eq!(summary::percentile(&v, 100), 4.0);
    assert_eq!(summary::percentile(&[], 50), 0.0);
}

#[test]
fn a_missing_or_empty_log_reports_no_records() {
    let d = scratch("empty");
    let empty = d.join("claude-auto-2026-11-01.jsonl");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(summary::report(&[empty, d.join("missing.jsonl")]), ["no timing records"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn log_files_walk_back_across_a_month_end() {
    let f = summary::log_files(std::path::Path::new("r"), (2028, 3, 1), 3);
    let names: Vec<String> = f.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(
        names,
        [
            "claude-auto-2028-03-01.jsonl",
            "claude-auto-2028-02-29.jsonl",
            "claude-auto-2028-02-28.jsonl"
        ]
    );
}

#[test]
fn timings_arguments_parse_strictly() {
    let a = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert_eq!(summary::parse_args(&a(&["--timings"])), Some(Ok(1)));
    assert_eq!(summary::parse_args(&a(&["--timings", "--days", "7"])), Some(Ok(7)));
    assert!(matches!(summary::parse_args(&a(&["--timings", "--days", "0"])), Some(Err(_))));
    assert!(matches!(summary::parse_args(&a(&["--timings", "x"])), Some(Err(_))));
    assert_eq!(summary::parse_args(&a(&["-p", "--timings"])), None);
}

#[test]
fn debug_is_the_lone_flag_or_the_variable() {
    let a = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
    assert!(debug_requested(&a(&["--debug"]), None));
    assert!(debug_requested(&[], Some("1")));
    assert!(!debug_requested(&a(&["--debug", "-p", "x"]), None));
    assert!(!debug_requested(&[], Some("0")));
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
