use super::*;

/// 2026-09-16 12:00 local at offset 0.
const GOLDEN_NOW: i64 = 1_789_560_000_000;

fn golden_sessions() -> Vec<Session> {
    (1..=5i64)
        .map(|i| Session {
            session_id: format!("g{i}"),
            project: format!("project-{i}"),
            worktree: if i == 3 { "feature".into() } else { String::new() },
            title: format!("question {i}"),
            last_user: format!("snippet {i} about the thing that was asked"),
            last_assistant: "reply words ".repeat(usize::try_from(4 * i).unwrap()),
            modified_ms: GOLDEN_NOW - 7 * i * 60_000,
            prompt_count: 3 * i,
            size_bytes: 2048 * u64::try_from(i).unwrap(),
            ..Session::default()
        })
        .collect()
}

fn view<'a>(s: &'a [&'a Session], index: usize, w: usize, h: usize) -> PickerView<'a> {
    PickerView {
        sessions: s,
        index,
        filter: "",
        width: w,
        height: h,
        clock: Clock {
            now_ms: GOLDEN_NOW,
            offset_minutes: 0,
        },
        style: Style {
            color: false,
            ascii: false,
        },
        scope: PickerScope::None,
        project_name: "",
        hover: -1,
        hover_row: -1,
    }
}

fn golden(name: &str) -> String {
    let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn assert_golden(name: &str, actual: &str) {
    let expected = golden(name);
    let first = expected.lines().zip(actual.lines()).position(|(e, a)| e != a);
    assert_eq!(expected, actual, "{name}: first difference at line {first:?}");
}

#[test]
fn golden_clock_is_the_fixture_time() {
    assert_eq!(local_stamp(GOLDEN_NOW, 0), "16 Sep 12:00");
}

#[test]
fn wide_frames_match_the_powershell_goldens() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let mut memo = Memo::default();
    assert_golden("picker-100x30", &picker_frame(&view(&s, 1, 100, 30), &mut memo).lines.join("\n"));
    assert_golden("picker-100x24", &picker_frame(&view(&s, 0, 100, 24), &mut memo).lines.join("\n"));
    let colour = PickerView {
        scope: PickerScope::Project,
        project_name: "alpha",
        hover_row: 1,
        hover: 1,
        style: Style { color: true, ascii: false },
        ..view(&s, 2, 120, 30)
    };
    assert_golden(
        "picker-120x30-project-color",
        &picker_frame(&colour, &mut Memo::default()).lines.join("\n"),
    );
}

#[test]
fn memo_repaint_equals_a_rebuild() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    for (w, h, ascii) in [(120, 30, false), (80, 30, true), (80, 24, false), (198, 24, true)] {
        let base = PickerView {
            style: Style { color: true, ascii },
            ..view(&s, 1, w, h)
        };
        let mut memo = Memo::default();
        picker_frame(&base, &mut memo);
        for (hr, hv) in [(2, -1), (0, 2), (-1, -1), (4, 0)] {
            let v = PickerView {
                hover_row: hr,
                hover: hv,
                ..base.clone()
            };
            let key = memo_key(&v);
            assert_eq!(memo.key, key, "the hover is not part of the key");
            let repainted = picker_frame(&v, &mut memo);
            let rebuilt = picker_frame(&v, &mut Memo::default());
            assert_eq!(repainted, rebuilt, "{w}x{h} hover {hr}/{hv}");
        }
    }
}

#[test]
fn every_size_fits_with_its_headroom_row() {
    let owned: Vec<Session> = (0..40)
        .map(|i| Session {
            session_id: format!("s{i}"),
            project: format!("project-{i}"),
            last_user: "words ".repeat(30),
            last_assistant: "reply ".repeat(60),
            modified_ms: GOLDEN_NOW - i * 60_000,
            prompt_count: 2,
            ..Session::default()
        })
        .collect();
    let s: Vec<&Session> = owned.iter().collect();
    for w in [50, 60, 80, 99, 100, 120, 198] {
        for h in [16, 20, 24, 29, 30, 40] {
            for (scope, ascii) in [(PickerScope::None, false), (PickerScope::Project, true)] {
                let v = PickerView {
                    scope,
                    style: Style { color: false, ascii },
                    ..view(&s, 39, w, h)
                };
                let f = picker_frame(&v, &mut Memo::default());
                assert!(f.lines.len() < h, "{w}x{h}: {} lines leave the headroom row", f.lines.len());
                assert!(
                    f.lines.iter().all(|l| crate::layout::display_width(l) < w),
                    "{w}x{h}: a line past column {w}"
                );
                assert!(f.lines[0].contains("sessions"), "{w}x{h}: a real render, not the stub");
                if w < 100 {
                    let list = f.map.list.unwrap_or_default();
                    let preview = if h < 30 { 2 } else { 6 };
                    let want = h - 3 - f.map.footer_lines - preview;
                    assert_eq!(
                        usize::try_from(list.row_count).unwrap(),
                        want,
                        "{w}x{h}: the list takes what the preview gave up"
                    );
                }
            }
        }
    }
}

#[test]
fn a_band_that_does_not_fit_rebuilds() {
    let mut memo = Memo {
        key: 7,
        unpainted: vec!["ab".into()],
        painted: vec!["ab".into()],
        hover_row: -1,
        rows: vec![(0, Band { y: 0, start: 1, len: 5 })],
        ..Memo::default()
    };
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let v = PickerView {
        hover_row: 0,
        ..view(&s, 0, 100, 30)
    };
    assert!(memo.hit(7, &v, glyphs(false)).is_none());
}

#[test]
fn narrow_layout_preview_shrinks_below_thirty_rows() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let tall = picker_frame(&view(&s, 0, 80, 30), &mut Memo::default());
    let short = picker_frame(&view(&s, 0, 80, 24), &mut Memo::default());
    assert!(tall.lines.iter().any(|l| l.contains("──────────")), "a rule under the preview head");
    assert!(tall.lines.iter().all(|l| crate::layout::display_width(l) <= 79));
    assert!(short.lines.len() <= 24 && tall.lines.len() <= 30);
    assert!(short.lines.iter().any(|l| l.contains("3 msgs")));
    let map = tall.map.list.unwrap();
    assert_eq!((map.first_row_y, map.start), (1, 0));
}

#[test]
fn empty_and_hidden_counts_are_stated() {
    let mut owned = golden_sessions();
    owned[0].prompt_count = 0;
    owned[1].headless = true;
    let s: Vec<&Session> = owned.iter().collect();
    let f = picker_frame(&view(&s, 0, 100, 30), &mut Memo::default());
    assert!(
        f.lines[0].contains("3 sessions ─ 1 empty hidden ─ 1 headless hidden"),
        "{}",
        f.lines[0]
    );
    let filtered = PickerView {
        filter: "zzz",
        ..view(&s, 0, 100, 30)
    };
    assert!(
        picker_frame(&filtered, &mut Memo::default())
            .lines
            .iter()
            .any(|l| l.contains("nothing matches that filter"))
    );
    let none: Vec<&Session> = Vec::new();
    assert!(
        picker_frame(&view(&none, 0, 100, 30), &mut Memo::default())
            .lines
            .iter()
            .any(|l| l.contains("no sessions found"))
    );
    let small = picker_frame(&view(&s, 0, 40, 10), &mut Memo::default());
    assert!(small.lines[0].contains("need 50x16"));
}

#[test]
fn relative_age_and_counts() {
    let now = 1_000_000_000_000;
    assert_eq!(relative_age(now - 20_000, now), "now");
    assert_eq!(relative_age(now - 5 * 60_000, now), "5 min");
    assert_eq!(relative_age(now - 3 * 3_600_000, now), "3 h");
    assert_eq!(relative_age(now - 2 * 86_400_000, now), "2 d");
    assert_eq!(n0(1234.5), "1,235");
    let capped = Session {
        prompt_count: 3,
        prompt_count_capped: true,
        ..Session::default()
    };
    assert_eq!(prompt_count_text(&capped), "3+");
}

#[test]
fn exchange_falls_back_and_attributes_by_word() {
    let bare = Session::default();
    let e = exchange(&bare);
    assert_eq!((e[0].text.as_str(), e[1].text.as_str()), ("(nothing was asked)", "(no reply)"));
    let g = glyphs(true);
    let lines = exchange_lines(&e, 40, 10, &g, true);
    assert_eq!(lines, ["you > (nothing was asked)", "claude > (no reply)"]);
}
