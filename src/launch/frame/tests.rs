#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use super::*;
use crate::launch::tests::four;

mod page;

const NOW_LABELS: [(&str, &str); 4] = [
    ("Model", "Opus 5.5[1M]"),
    ("Effort", "high"),
    ("Permission", "auto"),
    ("Advisor", "default"),
];

fn golden_limits() -> Limits {
    let rec = |f, s, age: &str, m: Option<i64>, ml: Option<&str>| LimitRecord {
        five_hour: Some(f),
        seven_day: Some(s),
        age_text: age.into(),
        model: m,
        model_label: ml.map(String::from),
        model_age: None,
        available_models: None,
    };
    [
        ("work".to_owned(), rec(41, 63, "12 min ago", Some(15), Some("FABLE"))),
        ("personal".to_owned(), rec(17, 14, "1 h ago", None, None)),
        ("low".to_owned(), rec(43, 22, "just now", None, None)),
    ]
    .into()
}

fn real_state(r: &Roster) -> LaunchState {
    let mut s = r.new_state();
    s.model = "opus1m".into();
    s.effort = "xhigh".into();
    s.advisor = "fable".into();
    s.permission = "bypass".into();
    s.restored = ["Model", "Effort", "Advisor", "Permission"].map(String::from).to_vec();
    s.restored_age = "2 min".into();
    s
}

fn golden(name: &str) -> String {
    let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

struct Case<'a> {
    state: &'a LaunchState,
    width: usize,
    height: usize,
    limits: &'a Limits,
    version: &'a VersionInfo,
    labels: &'a DefaultLabels,
    model_label: &'a str,
    style: Style,
}

fn render(r: &Roster, c: &Case<'_>) -> String {
    let inp = LaunchFrameInput {
        state: c.state,
        width: c.width,
        height: c.height,
        limits: c.limits,
        version: c.version,
        restored: None,
        notice: None,
        warnings: 0,
        recent: &[],
        running: &[],
        default_model_label: c.model_label,
        default_advisor_label: "default",
        default_labels: c.labels,
        style: c.style,
    };
    launch_frame(r, &inp).lines.join("\n")
}

/// Compares with the golden; `CLAUDE_AUTO_BLESS` set rewrites it instead (review the diff).
fn assert_golden(name: &str, actual: &str) {
    if std::env::var_os("CLAUDE_AUTO_BLESS").is_some() {
        let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = golden(name);
    let first_diff = expected.lines().zip(actual.lines()).position(|(e, a)| e != a);
    assert_eq!(expected, actual, "{name}: first difference at line {first_diff:?}");
}

#[test]
fn wide_frames_match_the_powershell_goldens() {
    let r = four();
    let limits = golden_limits();
    let none = Limits::new();
    let version = VersionInfo {
        installed: Some("2.1.226".into()),
        newest: Some("2.1.230".into()),
    };
    let no_version = VersionInfo::default();
    let labels: DefaultLabels = NOW_LABELS.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    let no_labels = DefaultLabels::new();
    let fresh = r.new_state();
    let real = real_state(&r);
    let mut hover = r.new_state();
    hover.row = 3;
    hover.hover = 1;
    hover.hover_row = 3;
    hover.hover_value = "max".into();
    let long_bucket: Limits = [(
        "work".to_owned(),
        LimitRecord {
            five_hour: Some(88),
            seven_day: Some(5),
            age_text: "3 d ago".into(),
            model: Some(61),
            model_label: Some("BUCKET-".repeat(9)),
            model_age: None,
            available_models: None,
        },
    )]
    .into();
    let plain = Style {
        color: false,
        ascii: false,
    };
    let color = Style { color: true, ascii: false };
    let ascii = Style { color: false, ascii: true };
    let base = Case {
        state: &fresh,
        width: 100,
        height: 30,
        limits: &none,
        version: &no_version,
        labels: &no_labels,
        model_label: "default",
        style: plain,
    };
    let real_case = Case {
        state: &real,
        limits: &limits,
        version: &version,
        labels: &labels,
        ..base
    };
    let cases: [(&str, Case<'_>); 6] = [
        ("launch-100x30", Case { ..base }),
        ("launch-100x30-real", Case { ..real_case }),
        ("launch-100x30-real-color", Case { style: color, ..real_case }),
        (
            "launch-120x30-real-ascii",
            Case {
                width: 120,
                style: ascii,
                ..real_case
            },
        ),
        (
            "launch-120x30-hover-color",
            Case {
                state: &hover,
                width: 120,
                limits: &limits,
                model_label: "default (Fable 5.1[1M])",
                style: color,
                ..base
            },
        ),
        (
            "launch-100x30-stacked",
            Case {
                limits: &long_bucket,
                ..base
            },
        ),
    ];
    for (name, c) in &cases {
        assert_golden(name, &render(&r, c));
    }
}

#[test]
fn narrow_tier_draws_compact_rows_inside_the_frame() {
    let r = four();
    let limits = golden_limits();
    let s = r.new_state();
    let labels = DefaultLabels::new();
    let v = VersionInfo::default();
    let inp = LaunchFrameInput {
        state: &s,
        width: 50,
        height: MIN_HEIGHT,
        limits: &limits,
        version: &v,
        restored: None,
        notice: None,
        warnings: 0,
        recent: &[],
        running: &[],
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style { color: false, ascii: true },
    };
    let f = launch_frame(&r, &inp);
    assert!(f.lines.iter().all(|l| display_width(l) <= 49), "{:#?}", f.lines);
    assert!(
        f.lines.iter().any(|l| l.starts_with("|   effort       < default >")),
        "too wide rows collapse: {:#?}",
        f.lines
    );
    assert!(
        f.lines.iter().any(|l| l.starts_with("|   mode         * [normal] - safe")),
        "a row that fits stays a radio list: {:#?}",
        f.lines
    );
    assert!(f.lines.len() <= MIN_HEIGHT, "the narrow worst case fits the minimum height");
}

fn frame_at(r: &Roster, s: &LaunchState, limits: &Limits, width: usize) -> Frame {
    let labels = DefaultLabels::new();
    let v = VersionInfo::default();
    let inp = LaunchFrameInput {
        state: s,
        width,
        height: 30,
        limits,
        version: &v,
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
    launch_frame(r, &inp)
}

/// The value row named `name`, as drawn.
fn row_line<'f>(f: &'f Frame, name: &str) -> &'f str {
    let hit = f.map.rows.iter().find(|h| h.name == name).unwrap();
    &f.lines[usize::try_from(hit.y).unwrap()]
}

#[test]
fn a_narrow_terminal_collapses_only_the_rows_that_do_not_fit() {
    let r = four();
    let limits = golden_limits();
    let s = r.new_state();
    for width in [50, 58, 60, 70, 78, 80, 90, 99] {
        let f = frame_at(&r, &s, &limits, width);
        for hit in f.map.rows.iter().filter(|h| h.name != "Account") {
            let line = &f.lines[usize::try_from(hit.y).unwrap()];
            assert!(
                line.contains('‹') != line.contains('●'),
                "{width}: collapsed or a radio list: {line}"
            );
        }
        assert!(f.lines.iter().all(|l| display_width(l) < width), "{width}: {:#?}", f.lines);
    }
    // Given 78 columns: effort's seven values do not fit, mode's two do.
    let f = frame_at(&r, &s, &limits, 78);
    assert!(row_line(&f, "Effort").contains('‹'), "{:#?}", f.lines);
    assert!(row_line(&f, "Mode").contains("● [normal] ○ safe"), "{:#?}", f.lines);
}

#[test]
fn narrow_and_wide_frames_match_their_goldens() {
    let plain = four();
    let mut action = four();
    action.add_action_row();
    let s = plain.new_state();
    let none = Limits::new();
    for (name, r, width) in [
        ("launch-78x30", &plain, 78),
        ("launch-78x30-action", &action, 78),
        ("launch-100x30-action", &action, 100),
    ] {
        let lines = frame_at(r, &r.new_state(), &none, width).lines;
        assert_golden(name, &lines.join("\n"));
    }
    if std::env::var_os("CLAUDE_AUTO_BLESS").is_none() {
        assert_eq!(
            frame_at(&plain, &s, &none, 100).lines.join("\n"),
            golden("launch-100x30"),
            "100 columns: nothing collapses"
        );
    }
}

#[test]
fn the_limits_panel_shows_the_selected_account_one_limit_per_line_and_the_age_last() {
    let r = four();
    let personal: Limits = [(
        "work".to_owned(),
        LimitRecord {
            five_hour: None,
            seven_day: Some(83),
            age_text: "just now".into(),
            model: Some(1),
            model_label: Some("FABLE".into()),
            model_age: None,
            available_models: None,
        },
    )]
    .into();
    let s = r.new_state();
    let wide = frame_at(&r, &s, &golden_limits(), 160);
    assert!(wide.lines[0].contains("limits \u{b7} work"), "{}", wide.lines[0]);
    let right = |f: &Frame, i: usize| f.lines[i].chars().skip(82).collect::<String>();
    let first = right(&wide, 1);
    assert!(first.starts_with("\u{2502}  5h ") && first.ends_with(" 41%  \u{2502}"), "{first}");
    assert!(right(&wide, 2).contains("7d") && right(&wide, 3).contains("15%"));
    let launch_h = r.rows.len() + 2;
    assert!(
        right(&wide, launch_h - 2).contains("data 12 min ago"),
        "the age is on the last row: {:#?}",
        wide.lines
    );
    let no_five = frame_at(&r, &s, &personal, 160);
    assert!(
        right(&no_five, 1).contains("7d") && right(&no_five, 1).contains("83%"),
        "{}",
        right(&no_five, 1)
    );
    assert!(!no_five.lines.iter().any(|l| l.contains(" 5h ")), "a missing limit draws no line");
    assert!(right(&frame_at(&r, &s, &Limits::new(), 160), 1).contains("no data yet"));
    // Stacked under 120 columns: the limits on one line, the age at its end.
    let narrow = frame_at(&r, &s, &golden_limits(), 100);
    let line = &narrow.lines[launch_h + 1];
    assert!(
        line.contains("5h 41%  7d 63%  fable 15%") && line.contains("data 12 min ago"),
        "{:#?}",
        narrow.lines
    );
    let y = |name: &str| wide.map.rows.iter().find(|h| h.name == name).unwrap().y;
    assert_eq!(y("Remote"), y("Account") + 1, "the account row takes one line");
}

#[test]
fn ctrl_r_reset_and_the_warning_badge_show_only_when_they_apply() {
    let r = four();
    let g = theme::glyphs(false);
    let text = |extra: LaunchHints| {
        launch_footer(&g, 159, false, &extra)
            .lines
            .iter()
            .map(|l| l.text.clone())
            .collect::<String>()
    };
    let none = text(LaunchHints::default());
    assert!(!none.contains("ctrl+r") && !none.contains('\u{26a0}'), "{none}");
    let both = text(LaunchHints {
        warnings: 2,
        restored: true,
        recent: false,
    });
    assert!(
        both.contains("[u] maintenance \u{26a0} 2") && both.contains("ctrl+r reset"),
        "{both}"
    );
    let colored = launch_footer(
        &g,
        159,
        true,
        &LaunchHints {
            warnings: 2,
            restored: false,
            recent: false,
        },
    );
    let line: String = colored
        .lines
        .iter()
        .map(|l| frame::add_hint_color(&l.text, &l.spans, true, &FooterLight::default()))
        .collect();
    assert!(
        line.contains(&format!("{}\u{26a0} 2", theme::YELLOW)),
        "the badge is yellow: {line:?}"
    );
    let f = frame_at(&r, &real_state(&r), &Limits::new(), 120);
    assert!(f.lines.iter().any(|l| l.contains("ctrl+r reset")), "restored rows offer the reset");
    assert!(!f.lines.iter().any(|l| l.contains("restored (")), "no restored status line");
    assert!(f.lines.iter().any(|l| l.contains("model*")), "the restored rows keep their mark");
}

#[test]
fn row_map_points_at_the_drawn_lines() {
    let r = four();
    let limits = golden_limits();
    let s = r.new_state();
    let labels = DefaultLabels::new();
    let v = VersionInfo::default();
    let inp = LaunchFrameInput {
        state: &s,
        width: 100,
        height: 30,
        limits: &limits,
        version: &v,
        restored: None,
        notice: None,
        warnings: 0,
        recent: &[],
        running: &[],
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style { color: false, ascii: true },
    };
    let f = launch_frame(&r, &inp);
    for hit in &f.map.rows {
        let line = f.lines.get(usize::try_from(hit.y).unwrap()).unwrap();
        assert!(line.contains(&r.rows[hit.index].label), "{} not on {line}", hit.name);
        for c in &hit.cells {
            let drawn: String = line
                .chars()
                .skip(usize::try_from(c.start).unwrap())
                .take(usize::try_from(c.end - c.start + 1).unwrap())
                .collect();
            assert!(drawn.contains(&c.value) || hit.name == "Model", "{drawn} vs {}", c.value);
        }
    }
    assert_eq!(f.map.footer.len(), 3, "enter, u, esc are clickable");
}

#[test]
fn footer_names_no_row_editing_keys() {
    let r = four();
    let s = r.new_state();
    let limits = golden_limits();
    let base = ["w/s row", "switch", "[enter] next", "[u] maint", "[esc] quit"];
    for width in [100, 120] {
        let f = frame_at(&r, &s, &limits, width);
        let text = f.lines[usize::try_from(f.map.footer_y.unwrap()).unwrap()..].join("\n");
        assert!(base.iter().all(|h| text.contains(h)), "{width}: {text}");
        assert!(!["^w/^s", "^x", "[o]"].iter().any(|h| text.contains(h)), "{width}: {text}");
    }
}

#[test]
fn under_110_columns_the_footer_shortens_and_stays_one_line_ctrl_r_going_first() {
    let g = theme::glyphs(false);
    let all = LaunchHints {
        warnings: 2,
        restored: true,
        recent: true,
    };
    let text = |w: usize| {
        let f = launch_footer(&g, w, false, &all);
        assert_eq!(f.lines.len(), 1, "{w}: {:?}", f.lines);
        f.lines[0].text.clone()
    };
    let wide = text(159);
    assert!(
        wide.contains("1-5 resume") && wide.contains("ctrl+r reset") && wide.contains("[u] maintenance"),
        "{wide}"
    );
    let mid = text(99);
    assert!(
        mid.contains("1-5 go") && mid.contains("[u] maint ") && mid.contains("a/d switch") && mid.contains("^r reset"),
        "{mid}"
    );
    let tight = text(90);
    assert!(!tight.contains("reset") && tight.contains("w/s row"), "ctrl+r goes first: {tight}");
    let narrow = text(79);
    assert!(narrow.contains("1-5 go") && !narrow.contains("w/s"), "{narrow}");
}

#[test]
fn model_row_weak_to_strong_and_fable_cell() {
    let r = four();
    let s = r.new_state();
    let mut limits = golden_limits();
    limits.get_mut("work").unwrap().model_age = Some("2 h ago".into());
    let f = frame_at(&r, &s, &limits, 100);
    let hit = f.map.rows.iter().find(|h| h.name == "Model").unwrap();
    let line = &f.lines[usize::try_from(hit.y).unwrap()];
    let at = |label: &str| line.find(label).unwrap_or_else(|| panic!("{label} missing: {line}"));
    assert!(
        at("default") < at("Haiku 5.5")
            && at("Haiku 5.5") < at("Sonnet 5.5")
            && at("Sonnet 5.5") < at("Opus 5.5[1M]")
            && at("Opus 5.5[1M]") < at("Fable 5.2"),
        "haiku, sonnet, opus, fable: {line}"
    );
    assert!(f.lines.iter().any(|l| l.contains("fable (2 h ago) 15%")), "{:#?}", f.lines);

    let mut ordered = four();
    ordered.apply_model_order(&["fable".to_owned(), "haiku".to_owned()]);
    assert_eq!(
        ordered.row(RowName::Model).unwrap().values,
        ["default", "fable", "haiku"],
        "modelOrder: listed families in its order, the rest hidden"
    );
}

#[test]
fn too_small_terminal_gets_the_size_frame() {
    let r = four();
    let s = r.new_state();
    let (l, v, d) = (Limits::new(), VersionInfo::default(), DefaultLabels::new());
    let inp = LaunchFrameInput {
        state: &s,
        width: 40,
        height: 10,
        limits: &l,
        version: &v,
        restored: None,
        notice: None,
        warnings: 0,
        recent: &[],
        running: &[],
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &d,
        style: Style { color: false, ascii: true },
    };
    assert_eq!(
        launch_frame(&r, &inp).lines.first().map(String::as_str),
        Some("  need 50x16, have 40x10")
    );
}
