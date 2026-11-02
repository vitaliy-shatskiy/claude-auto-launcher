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

fn project_hints() -> Vec<Hint> {
    vec![
        Hint::plain("w/s", "move"),
        Hint::plain("a/d", "action"),
        Hint::key("enter", "run", "Enter"),
        Hint::ch("c", "continue", 'c'),
        Hint::ch("r", "resume", 'r'),
        Hint::ch("t", "worktree", 't'),
        Hint::ch("/", "filter", '/'),
        Hint::key("esc", "back", "Escape"),
    ]
}

#[test]
fn a_narrow_footer_drops_the_plain_hints_and_keeps_one_line() {
    let f = new_hint_footer(&launch_hints(), &glyphs(true), 49, true);
    assert_eq!(
        f.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["  [enter] next  :  [u] maintenance  :  [esc] quit"]
    );
    assert_eq!(f.lines.first().unwrap().spans.len(), 3);
}

#[test]
fn a_footer_too_wide_for_its_buttons_wraps_into_aligned_columns() {
    for width in [40, 49, 59] {
        let f = new_hint_footer(&project_hints(), &glyphs(true), width, true);
        assert!(f.lines.len() >= 2, "{width}");
        assert!(f.lines.iter().all(|l| display_width(&l.text) <= width), "{width}: {f:#?}");
        let seps = |t: &str| t.char_indices().filter(|(_, c)| *c == ':').map(|(i, _)| i).collect::<Vec<_>>();
        let first = seps(&f.lines.first().unwrap().text);
        for l in &f.lines {
            let s = seps(&l.text);
            assert_eq!(s, first.get(..s.len()).unwrap(), "{width}: separators line up: {f:#?}");
            for span in &l.spans {
                let key: String = l
                    .text
                    .chars()
                    .skip(span.key_start)
                    .take(span.key_end - span.key_start + 1)
                    .collect();
                assert!(key.starts_with('[') && key.ends_with(']'), "{width}: span on its key {key}");
            }
        }
        assert!(f.lines.iter().flat_map(|l| &l.spans).all(|s| s.start >= 0), "only buttons are kept");
        assert_eq!(
            f.lines.iter().map(|l| l.spans.len()).sum::<usize>(),
            6,
            "{width}: every button kept"
        );
        assert_eq!(f.lines.last().unwrap().spans.last().unwrap().line, f.lines.len() - 1);
    }
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
