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
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style {
            color: false,
            ascii: false,
        },
    };
    let f = launch_frame(r, &inp);
    let placed = area.place(f.lines.clone());
    (f, placed)
}

#[test]
fn a_160x40_page_spans_the_width_with_the_footer_on_the_last_row() {
    let r = four();
    let (f, lines) = page(&r, &real_state(&r), (160, 40));
    assert_golden("launch-160x40", &lines.join("\n"));
    assert_eq!(lines.len(), 39, "every row but the headroom one");
    assert_eq!(display_width(&lines[0]), 159, "the header box spans the frame width");
    assert!(lines[38].contains("[esc] quit"), "{:?}", lines[38]);
    assert_eq!(f.map.footer_y, Some(38), "the footer's clicks follow it");
}

#[test]
fn a_400x150_terminal_draws_a_centred_240x80_page() {
    let r = four();
    let (_, lines) = page(&r, &real_state(&r), (400, 150));
    assert_golden("launch-400x150", &lines.join("\n"));
    assert_eq!(lines.len(), 79);
    let pad = " ".repeat(80);
    assert!(lines.iter().all(|l| l.is_empty() || l.starts_with(&pad)), "centred");
    assert!(lines.iter().all(|l| display_width(l) <= 80 + 239), "nothing past the area");
}

#[test]
fn the_switch_hint_sits_on_the_cursor_row_only() {
    let r = four();
    let mut s = real_state(&r);
    s.row = 3;
    let (f, lines) = page(&r, &s, (160, 40));
    let with_hint: Vec<usize> = (0..lines.len()).filter(|i| lines[*i].contains("a/d \u{2190}/\u{2192}")).collect();
    let effort = f.map.rows.iter().find(|h| h.name == "Effort").unwrap();
    assert_eq!(with_hint, [usize::try_from(effort.y).unwrap()], "{lines:#?}");
}
