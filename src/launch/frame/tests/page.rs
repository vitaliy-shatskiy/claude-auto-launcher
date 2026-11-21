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
        home: None,
        running: &[],
        mcp: "",
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
fn a_160x40_page_fills_the_terminal_and_ends_at_its_footer() {
    let r = four();
    let (f, lines) = page(&r, &real_state(&r), (160, 40));
    assert_golden("launch-160x40", &lines.join("\n"));
    assert_eq!(lines.len(), 39, "the panels run to the footer (AC27)");
    let footer = usize::try_from(f.map.footer_y.unwrap()).unwrap();
    assert!(
        lines.iter().take(footer).all(|l| display_width(l) == 159),
        "every panel row spans the width: {lines:#?}"
    );
    assert!(lines[footer].contains("[esc] quit"), "{:?}", lines[footer]);
}

#[test]
fn a_400x150_terminal_gets_a_full_width_page_from_the_top_left() {
    let r = four();
    let (_, lines) = page(&r, &real_state(&r), (400, 150));
    assert_golden("launch-400x150", &lines.join("\n"));
    assert!(lines.iter().all(|l| display_width(l) <= 399), "the last column stays free");
    assert!(lines[0].starts_with('\u{256D}'), "from the top-left corner");
}

fn sample_running() -> Vec<RunningSession> {
    [("claude-auto design", "busy", "12 min"), ("interactive 4120", "idle", "2 h")]
        .iter()
        .map(|(n, p, a)| RunningSession {
            name: (*n).into(),
            project: (*p).into(),
            age: (*a).into(),
        })
        .collect()
}

/// The cockpit page with running sessions and a newer Claude Code.
fn full(r: &Roster, state: &LaunchState, size: (usize, usize), running: &[RunningSession], limits: &Limits) -> Vec<String> {
    let version = VersionInfo {
        installed: Some("2.1.329".into()),
        newest: Some("2.1.330".into()),
    };
    let labels = DefaultLabels::new();
    let inp = LaunchFrameInput {
        state,
        width: size.0,
        height: size.1,
        limits,
        version: &version,
        restored: None,
        notice: None,
        warnings: 2,
        home: None,
        running,
        mcp: "2 of 2 \u{b7} 3 h ago",
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style {
            color: false,
            ascii: false,
        },
    };
    launch_frame(r, &inp).lines
}

#[test]
fn cockpit_goldens_at_130x40_80x24_and_for_a_new_user() {
    let r = four();
    let running = sample_running();
    let wide = full(&r, &real_state(&r), (130, 40), &running, &golden_limits());
    assert_golden("launch-130x40", &wide.join("\n"));
    assert_eq!(wide.len(), 39, "the panels stretch to the footer");
    let big = full(&r, &real_state(&r), (280, 72), &running, &golden_limits());
    assert_golden("launch-280x72", &big.join("\n"));
    let corner = big[0].chars().position(|c| c == '\u{256E}').unwrap() + 1;
    let at = |needle: &str| {
        let l = big.iter().find(|l| l.contains(needle)).unwrap();
        l.chars().count() - l.split_once(needle).unwrap().1.chars().count() - needle.chars().count()
    };
    assert_eq!(at("\u{256D}\u{2500} running"), corner, "running sits under limits, one column edge");
    assert_eq!(at("\u{256D}\u{2500} limits"), corner);
    assert_eq!(
        corner,
        279 - crate::cockpit::FACTS_WIDTH,
        "the facts column keeps its width, the left column takes the rest"
    );
    assert!(big.iter().all(|l| display_width(l) <= 279), "nothing past the frame");
    assert_golden(
        "launch-80x24",
        &full(&r, &real_state(&r), (80, 24), &running, &golden_limits()).join("\n"),
    );
    let s1 = full(&r, &r.new_state(), (130, 40), &[], &Limits::new());
    assert_golden("launch-130x40-s1", &s1.join("\n"));
    assert_eq!(s1.len(), 39, "empty lists keep the panels to the footer: {}", s1.len());
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
