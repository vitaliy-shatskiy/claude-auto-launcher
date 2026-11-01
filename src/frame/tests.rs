use super::*;
use crate::theme::glyphs;

fn launch_hints() -> Vec<Hint> {
    vec![
        Hint::plain("w/s", "row"),
        Hint::plain("a/d", "value"),
        Hint::key("enter", "next", "Enter"),
        Hint::ch("u", "maintenance", 'u'),
        Hint::key("esc", "quit", "Escape"),
    ]
}

#[test]
fn footer_plain_text_and_spans() {
    let f = new_hint_footer(&launch_hints(), &glyphs(true), 0, true);
    let l = f.lines.first().unwrap();
    assert_eq!(
        l.text,
        "  w/s row  :  a/d value  :  [enter] next  :  [u] maintenance  :  [esc] quit"
    );
    let u = l.spans.iter().find(|s| s.ch == Some('u')).unwrap();
    assert_eq!(l.text.chars().skip(u.key_start).take(3).collect::<String>(), "[u]");
    assert_eq!(l.spans.first().map(|s| s.start), Some(-1), "arrows are not clickable");
}

#[test]
fn footer_wraps_without_splitting_hints() {
    let f = new_hint_footer(&launch_hints(), &glyphs(true), 50, true);
    assert!(f.lines.len() >= 2);
    assert!(f.lines.iter().all(|l| display_width(&l.text) <= 50));
    assert_eq!(f.lines.last().unwrap().spans.last().unwrap().line, f.lines.len() - 1);
}

#[test]
fn hint_color_strips_back_to_plain() {
    let f = new_hint_footer(&launch_hints(), &glyphs(false), 0, false);
    let l = f.lines.first().unwrap();
    let light = FooterLight {
        hover: footer_hover(&f, 1),
        selected: Vec::new(),
    };
    let painted = add_hint_color(&l.text, &l.spans, true, &light);
    assert_eq!(theme::remove_ansi_color(&painted), l.text);
    assert!(painted.contains(&format!("{}{} u ", theme::ACCENT_BG, theme::ACCENT_FG)));
}

#[test]
fn radio_row_full_compact_and_unreadable() {
    let g = glyphs(true);
    let vals: Vec<String> = ["on", "off"].map(String::from).to_vec();
    let spec = RadioSpec {
        prefix: "   ",
        label: "remote",
        values: &vals,
        current: "on",
        texts: &vals,
        label_width: 12,
        max_width: 0,
        hover: "",
        compact: false,
    };
    let r = new_radio_row(&spec, &g);
    assert_eq!(r.text, "   remote      * [on] - off");
    assert_eq!(r.cells.first().map(|c| (c.start, c.end)), Some((15, 20)));
    let c = new_radio_row(
        &RadioSpec {
            compact: true,
            ..spec.clone()
        },
        &g,
    );
    assert_eq!(c.text, "   remote      [on] off");
    let spaced: Vec<String> = ["stop server", "on"].map(String::from).to_vec();
    let u = new_radio_row(
        &RadioSpec {
            values: &spaced,
            texts: &spaced,
            compact: true,
            ..spec
        },
        &g,
    );
    assert!(!u.compact, "a spaced label has no readable compact form");
}

#[test]
fn too_small_keeps_size_line_first() {
    let f = too_small_frame(40, 2, true);
    assert_eq!(f, ["  need 50x16, have 40x2", "  terminal too small"]);
}

#[test]
fn held_frame_keeps_hint_when_short() {
    let f = held_session_frame("another claude holds this session and the words go on", 60, 3, true);
    assert_eq!(f.last().map(String::as_str), Some("  enter continue anyway   esc back"));
    assert_eq!(f.len(), 2, "no blank lines when the full frame does not fit");
}
