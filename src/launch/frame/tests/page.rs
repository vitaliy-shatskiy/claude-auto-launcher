//! The launch screen as a full page: the width, the footer at the bottom, the area cap, the
//! cursor row's switch hint.

use super::*;
use crate::frame::Area;

/// The real-looking state drawn through the area rule, as the screen loop paints it.
fn page(r: &Roster, state: &LaunchState, size: (usize, usize)) -> (Frame, Vec<String>) {
    let limits = golden_limits();
    let version = VersionInfo {
        installed: Some("2.1.329".into()),
        newest: None,
    };
    let labels = DefaultLabels::new();
    let area = Area::of(size);
    let inp = LaunchFrameInput {
        state,
        width: area.width,
        height: area.height,
        limits: &limits,
        version: &version,
        restored: None,
        notice: None,
        warnings: 0,
        recent: &[],
        running: &[],
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style {
            color: false,
            ascii: false,
        },
    };
    let f = launch_frame(r, &inp);
    let lines = f.lines.clone();
    (f, lines)
}

#[test]
fn a_160x40_page_spans_the_width_and_ends_at_its_footer() {
    let r = four();
    let (f, lines) = page(&r, &real_state(&r), (160, 40));
    assert_golden("launch-160x40", &lines.join("\n"));
    assert!(lines.len() < 20, "content height, nothing pads to the bottom: {}", lines.len());
    assert_eq!(display_width(&lines[0]), 159, "the header box spans the frame width");
    let last = lines.len() - 1;
    assert!(lines[last].contains("[esc] quit"), "{:?}", lines[last]);
    assert_eq!(f.map.footer_y, Some(i32::try_from(last).unwrap()), "the footer's clicks follow it");
}

#[test]
fn a_400x150_terminal_gets_a_full_width_page_from_the_top_left() {
    let r = four();
    let (_, lines) = page(&r, &real_state(&r), (400, 150));
    assert_golden("launch-400x150", &lines.join("\n"));
    assert!(lines.len() < 20, "content height on a huge terminal too");
    assert_eq!(display_width(&lines[0]), 399, "the header box spans the terminal from column 0");
    assert!(lines.iter().all(|l| display_width(l) <= 399), "the last column stays free");
}

#[test]
fn the_switch_keys_sit_in_the_footer_only() {
    let r = four();
    let mut s = real_state(&r);
    s.row = 3;
    let (f, lines) = page(&r, &s, (160, 40));
    let with_keys: Vec<usize> = (0..lines.len()).filter(|i| lines[*i].contains("a/d")).collect();
    assert_eq!(with_keys, [usize::try_from(f.map.footer_y.unwrap()).unwrap()], "{lines:#?}");
    assert!(
        lines[with_keys[0]].contains("a/d \u{2190}/\u{2192} switch"),
        "{}",
        lines[with_keys[0]]
    );
}
