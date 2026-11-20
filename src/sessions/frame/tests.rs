use super::*;
use crate::sessions::chat::ChatMessage;
use crate::sessions::listing::Speaker;

/// 2026-09-16 12:00 local at offset 0.
const GOLDEN_NOW: i64 = 1_789_560_000_000;

fn golden_sessions() -> Vec<Session> {
    (1..=5i64)
        .map(|i| Session {
            session_id: format!("g{i}"),
            project: format!("project-{i}"),
            worktree: if i == 3 { "feature".into() } else { String::new() },
            title: format!("session title {i}"),
            first_prompt: format!("first question {i} about the start of it all"),
            message_count: 10 * i,
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
        chat: ChatView::Messages(golden_chat()),
    }
}

fn msg(speaker: Speaker, text: &str, minute: i64) -> ChatMessage {
    ChatMessage {
        speaker,
        text: text.into(),
        at_ms: Some(GOLDEN_NOW - (60 - minute) * 60_000),
    }
}

/// An exchange of the shapes the pane meets: short, wrapped, long, and a long newest reply.
fn golden_chat() -> &'static [ChatMessage] {
    static CHAT: std::sync::LazyLock<Vec<ChatMessage>> = std::sync::LazyLock::new(|| {
        vec![
            msg(Speaker::User, "first question about the start of it all", 1),
            msg(Speaker::Assistant, &"an older reply that is long enough to be cut ".repeat(12), 2),
            msg(Speaker::User, "make the picker preview a chat", 10),
            msg(
                Speaker::Assistant,
                "Plan: read the transcript tail on a worker thread, then render it.",
                11,
            ),
            msg(Speaker::User, "ok, do it", 12),
            msg(
                Speaker::Assistant,
                &format!(
                    "Done. {} The last line of the newest reply.",
                    "Every step is in the log. ".repeat(20)
                ),
                40,
            ),
        ]
    });
    &CHAT
}

fn golden(name: &str) -> String {
    let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn assert_golden(name: &str, actual: &str) {
    if std::env::var_os("CLAUDE_AUTO_BLESS").is_some() {
        let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = golden(name);
    let first = expected.lines().zip(actual.lines()).position(|(e, a)| e != a);
    assert_eq!(expected, actual, "{name}: first difference at line {first:?}");
}

#[test]
fn golden_clock_is_the_fixture_time() {
    assert_eq!(local_stamp(GOLDEN_NOW, 0), "16.09 12:00");
    assert_eq!(
        local_stamp(GOLDEN_NOW + 13 * 3_600_000, 120),
        "17.09 03:00",
        "the local offset crosses midnight"
    );
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
fn chat_frames_match_their_goldens_in_colour_and_ascii() {
    let mut owned = golden_sessions();
    owned[0].modified_ms = GOLDEN_NOW - 20_000;
    let s: Vec<&Session> = owned.iter().collect();
    let ascii = PickerView {
        style: Style { color: false, ascii: true },
        ..view(&s, 1, 120, 30)
    };
    assert_golden(
        "picker-120x30-chat-ascii",
        &picker_frame(&ascii, &mut Memo::default()).lines.join("\n"),
    );
    let colour = PickerView {
        scope: PickerScope::All,
        hover_row: 3,
        style: Style { color: true, ascii: false },
        ..view(&s, 1, 200, 60)
    };
    assert_golden(
        "picker-200x60-chat-color",
        &picker_frame(&colour, &mut Memo::default()).lines.join("\n"),
    );
    let loading = PickerView {
        chat: ChatView::Loading,
        ..view(&s, 1, 120, 30)
    };
    let f = picker_frame(&loading, &mut Memo::default());
    assert!(f.lines.iter().any(|l| l.contains("│ loading…")), "{:#?}", f.lines);
}

/// The right pane's text of a frame, right-trimmed only (the side a line sits on matters).
fn right_pane(f: &Frame) -> Vec<String> {
    f.lines
        .iter()
        .skip(1)
        .map(|l| l.rsplit('│').nth(1).unwrap_or("").trim_end().to_owned())
        .collect()
}

#[test]
fn the_chat_fills_from_the_bottom_user_right_claude_left_newest_shown_from_its_end() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let f = picker_frame(&view(&s, 1, 120, 30), &mut Memo::default());
    let right = right_pane(&f);
    assert_eq!(right[0].trim(), "session title 2");
    assert_eq!(right[1].trim(), "16.09 11:46 · 20 msgs · 4 KB · project-2");
    assert!(right[2].trim().chars().all(|c| c == '─'), "a rule: {:?}", right[2]);
    let chat: Vec<&String> = right
        .iter()
        .skip(3)
        .take_while(|l| !l.starts_with('─') && !l.contains("╰"))
        .collect();
    let last = chat.iter().rev().find(|l| !l.trim().is_empty()).unwrap();
    assert!(last.ends_with("the newest reply."), "the newest ends at the bottom: {chat:#?}");
    assert!(chat.iter().any(|l| l.contains(" claude · 16.09 11:40")), "{chat:#?}");
    let you = chat.iter().find(|l| l.trim_start().starts_with("you ·")).unwrap();
    let width = crate::layout::display_width(you);
    assert!(you.starts_with("    "), "the owner's label sits right: {you:?}");
    let prompt = chat.iter().find(|l| l.trim() == "ok, do it").unwrap();
    assert_eq!(crate::layout::display_width(prompt), width, "and his text ends at the same edge");
    assert!(!chat.iter().any(|l| l.contains("first question")), "the first prompt does not fit");
    // A newest reply taller than the pane shows its end, cut at the top with `…`.
    let mut long = golden_chat().to_vec();
    long.push(msg(Speaker::Assistant, &format!("START {} END", "word ".repeat(600)), 50));
    let f = picker_frame(
        &PickerView {
            chat: ChatView::Messages(&long),
            ..view(&s, 1, 120, 30)
        },
        &mut Memo::default(),
    );
    let right = right_pane(&f);
    assert!(right[3].starts_with(" claude · 16.09 11:50"), "{right:#?}");
    assert!(right[4].starts_with(" …"), "{right:#?}");
    assert!(right.iter().any(|l| l.ends_with("END")), "{right:#?}");
    assert!(!right.iter().any(|l| l.contains("START")), "{right:#?}");
}

#[test]
fn a_big_terminal_frame_fills_it_and_matches_its_golden() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let all = PickerView {
        scope: PickerScope::All,
        ..view(&s, 0, 200, 60)
    };
    let f = picker_frame(&all, &mut Memo::default());
    assert_golden("picker-200x60-all", &f.lines.join("\n"));
    assert_eq!(f.lines.len(), 59, "AC27: every row but the headroom one");
    assert_eq!(crate::layout::display_width(&f.lines[0]), 199, "and every column but the last");
}

#[test]
fn the_box_fills_the_terminal_whatever_the_session_count() {
    let owned = golden_sessions();
    let one: Vec<&Session> = owned.iter().take(1).collect();
    let none: Vec<&Session> = Vec::new();
    for (w, h) in [(100, 30), (200, 60), (80, 40)] {
        for s in [&one, &none] {
            let f = picker_frame(&view(s, 0, w, h), &mut Memo::default());
            assert_eq!(f.lines.len(), h - 1, "{w}x{h} with {} sessions", s.len());
        }
    }
}

/// AC5: what a cursor move costs on the console at 250x68 (a 240x68 area, five columns in).
#[test]
fn a_cursor_move_writes_a_fraction_of_a_full_frame() {
    let owned: Vec<Session> = (0..69i64)
        .map(|i| Session {
            session_id: format!("s{i}"),
            project: format!("project-{i}"),
            title: format!("session title {i} with some words"),
            modified_ms: GOLDEN_NOW - i * 3_600_000,
            prompt_count: 3,
            message_count: 40,
            size_bytes: 4_000_000,
            ..Session::default()
        })
        .collect();
    let s: Vec<&Session> = owned.iter().collect();
    let area = crate::frame::Area::of((250, 68));
    let base = PickerView {
        scope: PickerScope::All,
        style: Style { color: true, ascii: false },
        ..view(&s, 0, area.width, area.height)
    };
    let frame = |v: &PickerView<'_>| area.place(picker_frame(v, &mut Memo::default()).lines);
    let a = frame(&base);
    let hovered = frame(&PickerView {
        hover_row: 1,
        ..base.clone()
    });
    let moved = frame(&PickerView {
        index: 1,
        chat: ChatView::Loading,
        ..base.clone()
    });
    let full = crate::ui::frame_bytes(&a).len();
    let hover = crate::ui::frame_diff_bytes(&a, &hovered).len();
    let key = crate::ui::frame_diff_bytes(&a, &moved).len();
    eprintln!("bytes at 250x68: full frame {full}, hover move {hover}, key move {key}");
    assert!(hover * 10 < full, "a hover move rewrites two rows: {hover} of {full}");
    assert!(key < full, "a key move rewrites the rows and the pane: {key} of {full}");
}

#[test]
fn sizes_read_in_kb_then_mb_and_the_list_pane_stops_growing() {
    assert_eq!(card::size_text(2048), "2 KB");
    assert_eq!(card::size_text(1_697_792), "1.6 MB", "the owner's 1,658 KB session");
    assert_eq!(pane_widths(99, true).0, 39, "40 % at 100 columns");
    assert_eq!(pane_widths(249, true).0, MAX_LIST_WIDTH, "capped on a 250-column terminal");
    assert_eq!(
        list_capacity(
            200,
            60,
            PickerScope::All,
            Style {
                color: false,
                ascii: false
            }
        ),
        56
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
        for (hr, hv) in [(2, -1), (1, -1), (0, 2), (-1, -1), (4, 0)] {
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
        selected: -1,
        rows: vec![(0, Band { y: 0, start: 1, len: 5 })],
        ..Memo::default()
    };
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let v = PickerView {
        hover_row: 0,
        ..view(&s, 0, 100, 30)
    };
    assert!(memo.hit(7, &v).is_none());
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
    assert!(short.lines.iter().any(|l| l.contains("10 msgs")), "{:#?}", short.lines);
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
    assert_eq!(text::n0(1234.5), "1,235");
}
