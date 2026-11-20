use super::*;

fn panel(focus: Focus, lines: &[&str]) -> Panel<'static> {
    Panel {
        title: "✻ claude-auto".into(),
        page: String::new(),
        right: "0.9.0".into(),
        focus,
        lines: lines.iter().map(|s| (*s).to_owned()).collect(),
        width: 30,
        height: 4,
        painter: &no_paint,
    }
}

const PLAIN: Style = Style {
    color: false,
    ascii: false,
};
const COLOR: Style = Style { color: true, ascii: false };

#[test]
fn a_panel_is_exactly_its_size_with_the_title_and_right_text_in_the_border() {
    let l = render(&panel(Focus::Plain, &["one", "a line far longer than the interior is wide"]), PLAIN);
    assert_eq!(l.len(), 4);
    assert!(l.iter().all(|x| display_width(x) == 30), "{l:#?}");
    assert_eq!(l[0], "╭─ ✻ claude-auto ──── 0.9.0 ─╮");
    assert_eq!(l[1], "│one                         │");
    assert!(l[2].ends_with("…│"), "a long line is cut: {}", l[2]);
    assert_eq!(l[3], "╰────────────────────────────╯");
}

#[test]
fn the_right_text_goes_when_the_border_has_no_room() {
    let mut p = panel(Focus::Plain, &[]);
    p.right = "x".repeat(20);
    assert_eq!(render(&p, PLAIN)[0], "╭─ ✻ claude-auto ────────────╮");
}

#[test]
fn the_page_word_follows_the_title_after_a_rule() {
    let mut p = panel(Focus::Plain, &[]);
    p.page = "maintenance".into();
    p.right = String::new();
    p.width = 34;
    assert_eq!(render(&p, PLAIN)[0], "╭─ ✻ claude-auto ─ maintenance ──╮");
}

#[test]
fn focus_takes_the_accent_border_bold_title_and_the_lighter_tint() {
    let focused = render(&panel(Focus::Focused, &["a"]), COLOR);
    let plain = render(&panel(Focus::Plain, &["a"]), COLOR);
    assert!(focused[0].starts_with(ACCENT) && focused[0].contains(&format!("{ACCENT}{}", theme::BOLD)));
    assert!(focused[1].contains(TINT_FOCUSED) && !focused[1].contains(TINT));
    assert!(plain[0].starts_with(FAINT) && !plain[0].contains(theme::BOLD));
    assert!(plain[1].contains(TINT));
    assert_eq!(theme::remove_ansi_color(&focused[1]), theme::remove_ansi_color(&plain[1]));
}

#[test]
fn the_tint_comes_back_after_every_inner_reset() {
    let l = render(&panel(Focus::Plain, &["\u{1}dim\u{2} text"]), COLOR);
    assert!(l[1].contains(&format!("{}{TINT}", theme::RESET)), "{:?}", l[1]);
}

#[test]
fn ascii_and_no_colour_draw_no_tint() {
    let l = render(&panel(Focus::Focused, &["a"]), Style { color: false, ascii: true });
    assert_eq!(l[0], "+- ✻ claude-auto ---- 0.9.0 -+");
    assert!(!l.iter().any(|x| x.contains('\x1b')));
}

#[test]
fn beside_pads_the_shorter_left_column() {
    let out = beside(vec!["ab".into()], 2, &["1".to_owned(), "2".to_owned()]);
    assert_eq!(out, ["ab1", "  2"]);
}

#[test]
fn spread_right_aligns_or_drops_the_right_text() {
    assert_eq!(spread("a", "b", 5), "a   b");
    assert_eq!(spread("abcd", "ef", 5), "abcd");
}
