use std::time::{Duration, Instant};

use super::*;
use crate::layout::display_width;
use crate::timing::Timings;

fn at(origin: Instant, ms: u64) -> Instant {
    origin + Duration::from_millis(ms)
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
