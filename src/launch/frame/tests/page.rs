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
        recent_loading: false,
        recent_selected: None,
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
fn a_160x40_page_spans_the_width_and_ends_at_its_footer() {
    let r = four();
    let (f, lines) = page(&r, &real_state(&r), (160, 40));
    assert_golden("launch-160x40", &lines.join("\n"));
    assert!(lines.len() < 20, "content height, nothing pads to the bottom: {}", lines.len());
    let recent = lines.iter().find(|l| l.contains("recent")).unwrap();
    assert_eq!(
        display_width(recent),
        display_width(&lines[0]),
        "recent and the right column end where the top row does"
    );
    assert!(display_width(&lines[0]) < 159, "the launch row is as wide as its content");
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
    let recent = lines.iter().find(|l| l.contains("recent")).unwrap();
    assert_eq!(
        display_width(recent),
        display_width(&lines[0]),
        "one grid: recent ends where the top row does"
    );
    assert!(lines.iter().all(|l| display_width(l) <= 399), "the last column stays free");
}

fn sample_recent() -> Vec<RecentProject> {
    [
        ("aurora-web", "3 min", "claude-auto design variants"),
        ("invoice-api", "4 h", "fix the invoice export paging"),
        ("apk-diff", "1 d", ""),
        ("channel-monitor", "2 d", "restart loop on the VM"),
    ]
    .iter()
    .map(|(n, a, last)| RecentProject {
        name: (*n).into(),
        path: format!(r"C:\Users\sample\Desktop\Projects\{n}"),
        age: (*a).into(),
        last: (*last).into(),
        ..RecentProject::default()
    })
    .collect()
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

/// The cockpit page with recent projects, running sessions and a newer Claude Code.
fn full(
    r: &Roster,
    state: &LaunchState,
    size: (usize, usize),
    recent: &[RecentProject],
    running: &[RunningSession],
    limits: &Limits,
) -> Vec<String> {
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
        recent,
        recent_loading: false,
        recent_selected: None,
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
    let (recent, running) = (sample_recent(), sample_running());
    let wide = full(&r, &real_state(&r), (130, 40), &recent, &running, &golden_limits());
    assert_golden("launch-130x40", &wide.join("\n"));
    assert_eq!(wide.len(), 39, "lists with items stretch the panels to the footer");
    let big = full(&r, &real_state(&r), (280, 72), &recent, &running, &golden_limits());
    assert_golden("launch-280x72", &big.join("\n"));
    let corner = big[0].chars().position(|c| c == '\u{256E}').unwrap() + 1;
    let at = |needle: &str| {
        let l = big.iter().find(|l| l.contains(needle)).unwrap();
        l.chars().count() - l.split_once(needle).unwrap().1.chars().count() - needle.chars().count()
    };
    assert_eq!(at("\u{256D}\u{2500} running"), corner, "running sits under limits, one column edge");
    assert_eq!(at("\u{256D}\u{2500} limits"), corner);
    assert!(big.iter().all(|l| display_width(l) <= corner + 46), "nothing past the grid");
    assert!(corner < 100, "the launch panel ends at its rows, not at the far column: {corner}");
    assert!(
        big[0]
            .chars()
            .skip(corner)
            .collect::<String>()
            .starts_with("\u{256D}\u{2500} limits"),
        "limits right beside it"
    );
    assert_golden(
        "launch-80x24",
        &full(&r, &real_state(&r), (80, 24), &recent, &running, &golden_limits()).join("\n"),
    );
    let s1 = full(&r, &r.new_state(), (130, 40), &[], &[], &Limits::new());
    assert_golden("launch-130x40-s1", &s1.join("\n"));
    assert!(s1.len() < 25, "empty lists shrink the panels and the footer follows: {}", s1.len());
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
