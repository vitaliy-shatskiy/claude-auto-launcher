#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use super::*;
use crate::launch::tests::four;

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
        height: 30,
        limits: c.limits,
        version: c.version,
        restored: None,
        default_model_label: c.model_label,
        default_advisor_label: "default",
        default_labels: c.labels,
        style: c.style,
        notice: "",
    };
    launch_frame(r, &inp).lines.join("\n")
}

fn assert_golden(name: &str, actual: &str) {
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
fn narrow_tier_draws_compact_rows_and_one_limit_line() {
    let r = four();
    let limits = golden_limits();
    let s = r.new_state();
    let labels = DefaultLabels::new();
    let v = VersionInfo::default();
    let inp = LaunchFrameInput {
        state: &s,
        width: 50,
        height: 30,
        limits: &limits,
        version: &v,
        restored: None,
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style { color: false, ascii: true },
        notice: "",
    };
    let f = launch_frame(&r, &inp);
    assert!(f.lines.iter().all(|l| display_width(l) <= 49), "{:#?}", f.lines);
    assert!(
        f.lines.iter().any(|l| l == "   5h     41%   7d     63%   fable  15%"),
        "the usage row keeps the model bucket before the age: {:#?}",
        f.lines
    );
    assert!(
        f.lines.iter().any(|l| l == "   effort       < default >"),
        "too wide rows collapse: {:#?}",
        f.lines
    );
    assert!(
        f.lines.iter().any(|l| l == "   advisor      < default >"),
        "and every other value row with them: {:#?}",
        f.lines
    );
    assert!(f.lines.len() < MIN_HEIGHT, "the narrow worst case stays under the minimum height");
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
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style {
            color: false,
            ascii: false,
        },
        notice: "",
    };
    launch_frame(r, &inp)
}

/// The column where the values of the row named `name` start.
fn value_column(f: &Frame, r: &Roster, name: &str) -> usize {
    let hit = f.map.rows.iter().find(|h| h.name == name).unwrap();
    let line = &f.lines[usize::try_from(hit.y).unwrap()];
    let label = r.rows[hit.index].label;
    let chars: Vec<char> = line.chars().collect();
    let label: Vec<char> = label.chars().collect();
    let at = chars.windows(label.len()).position(|w| w == label.as_slice()).unwrap() + label.len();
    at + chars.iter().skip(at).take_while(|c| **c == ' ' || **c == '*').count()
}

#[test]
fn narrow_value_rows_collapse_all_or_none() {
    let r = four();
    let limits = golden_limits();
    let s = r.new_state();
    for width in [50, 58, 60, 70, 80, 90, 99] {
        let f = frame_at(&r, &s, &limits, width);
        let rows: Vec<&String> = f
            .map
            .rows
            .iter()
            .filter(|h| h.name != "Account")
            .map(|h| &f.lines[usize::try_from(h.y).unwrap()])
            .collect();
        let collapsed = rows.iter().filter(|l| l.contains('‹')).count();
        assert!(collapsed == 0 || collapsed == rows.len(), "{width}: {:#?}", f.lines);
        if collapsed == 0 {
            assert!(rows.iter().all(|l| l.contains('●')), "{width}: radio lists: {:#?}", f.lines);
        }
        assert!(f.lines.iter().all(|l| display_width(l) < width), "{width}: {:#?}", f.lines);
    }
}

#[test]
fn the_usage_row_starts_on_the_value_column_and_keeps_the_model_bucket() {
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
    for limits in [golden_limits(), personal] {
        for width in [50, 58, 60, 80, 100, 120] {
            let f = frame_at(&r, &s, &limits, width);
            let account_y = usize::try_from(f.map.rows.iter().find(|h| h.name == "Account").unwrap().y).unwrap();
            let usage = &f.lines[account_y + 1];
            let col = value_column(&f, &r, "Model");
            let lead = usage.chars().take_while(|c| *c == ' ').count();
            assert!(
                lead == col || (width < 100 && lead == 3),
                "{width}: value or label column: {usage:?}"
            );
            assert!(usage.contains("fable"), "{width}: {usage:?}");
            assert!(usage.contains("7d "), "{width}: {usage:?}");
            let l = &limits["work"];
            if width >= 58 {
                assert!(usage.contains("fable") && usage.ends_with(&l.age_text), "{width}: {usage:?}");
            }
            if width >= 80 || (width >= 58 && l.five_hour.is_none()) {
                assert!(usage.contains('░'), "{width}: bars kept: {usage:?}");
            }
        }
    }
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
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &labels,
        style: Style { color: false, ascii: true },
        notice: "",
    };
    let f = launch_frame(&r, &inp);
    for hit in &f.map.rows {
        let line = f.lines.get(usize::try_from(hit.y).unwrap()).unwrap();
        assert!(line.contains(r.rows[hit.index].label), "{} not on {line}", hit.name);
        for c in &hit.cells {
            let drawn: String = line
                .chars()
                .skip(usize::try_from(c.start).unwrap())
                .take(usize::try_from(c.end - c.start + 1).unwrap())
                .collect();
            assert!(drawn.contains(&c.value) || hit.name == "Model", "{drawn} vs {}", c.value);
        }
    }
    assert_eq!(f.map.footer.len(), 4, "enter, o, u, esc are clickable");
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
    let y = |name: &str| usize::try_from(f.map.rows.iter().find(|h| h.name == name).unwrap().y).unwrap();
    let usage = f.lines[y("Account") + 1..y("Remote")].join("\n");
    assert!(usage.contains("fable") && usage.contains("15% (2 h ago)"), "{usage}");

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
        default_model_label: "default",
        default_advisor_label: "default",
        default_labels: &d,
        style: Style { color: false, ascii: true },
        notice: "",
    };
    assert_eq!(
        launch_frame(&r, &inp).lines.first().map(String::as_str),
        Some("  need 50x16, have 40x10")
    );
}
