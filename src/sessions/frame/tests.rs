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
        typing: false,
        width: w,
        height: h,
        clock: Clock {
            now_ms: GOLDEN_NOW,
            zone: Zone(|_| 0),
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
fn cockpit_goldens_at_130x40_80x24_and_for_a_new_user() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    let wide = picker_frame(&view(&s, 1, 130, 40), &mut Memo::default()).lines;
    assert_golden("picker-130x40", &wide.join("\n"));
    assert!(
        wide[0].contains("resume \u{b7} 5 sessions") && wide[0].contains("chat \u{b7} project-2"),
        "{}",
        wide[0]
    );
    assert_golden(
        "picker-80x24",
        &picker_frame(&view(&s, 1, 80, 24), &mut Memo::default()).lines.join("\n"),
    );
    let none: Vec<&Session> = Vec::new();
    assert_golden(
        "picker-130x40-s1",
        &picker_frame(&view(&none, 0, 130, 40), &mut Memo::default()).lines.join("\n"),
    );
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
    owned[0].live = true;
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
fn the_chat_fills_from_the_bottom_user_right_claude_left_bodies_inset_each_from_its_start() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    // The chat panel at 140 columns is as wide as the old right pane at 120.
    let f = picker_frame(&view(&s, 1, 140, 30), &mut Memo::default());
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
    assert_eq!(
        crate::layout::display_width(prompt) + 2,
        width,
        "his text ends 2 cells in from that edge"
    );
    let reply = chat.iter().find(|l| l.contains("Plan: read")).unwrap();
    assert!(
        reply.starts_with("   Plan"),
        "Claude's body sits 2 cells in from his label: {reply:?}"
    );
    assert!(!chat.iter().any(|l| l.contains("first question")), "the first prompt does not fit");
    // A newest reply taller than the pane shows from its start, the rest counted.
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
    let at = right.iter().position(|l| l.starts_with(" claude · 16.09 11:50")).unwrap();
    assert!(right[at + 1].starts_with("   START"), "{right:#?}");
    assert!(right[at + 15].contains("more lines"), "{right:#?}");
    assert!(!right.iter().any(|l| l.contains("END")), "{right:#?}");
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

/// AC5: what a cursor move costs on the console at 250x68.
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
    let frame = |v: &PickerView<'_>| picker_frame(v, &mut Memo::default()).lines;
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
fn a_hover_repainted_from_the_memo_equals_a_fresh_frame_beside_and_stacked() {
    let owned = golden_sessions();
    let s: Vec<&Session> = owned.iter().collect();
    for width in [140, 100] {
        let base = PickerView {
            style: Style { color: true, ascii: false },
            ..view(&s, 0, width, 30)
        };
        let mut memo = Memo::default();
        let _ = picker_frame(&base, &mut memo);
        for hover_row in [2, 3, -1] {
            let v = PickerView { hover_row, ..base.clone() };
            let repainted = picker_frame(&v, &mut memo).lines;
            assert_eq!(
                repainted,
                picker_frame(&v, &mut Memo::default()).lines,
                "{width}: hover {hover_row}"
            );
        }
    }
}

#[test]
fn sizes_read_in_kb_then_mb_and_the_list_fills_the_page() {
    assert_eq!(card::size_text(2048), "2 KB");
    assert_eq!(card::size_text(1_697_792), "1.6 MB", "the owner's 1,658 KB session");
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
                assert!(f.lines[0].contains("resume"), "{w}x{h}: a real render, not the stub");
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
        f.lines[0].contains("3 sessions · 1 empty hidden · 1 headless hidden"),
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

#[test]
fn each_chat_stamp_takes_the_offset_of_its_own_date() {
    // Kyiv: +3 until the DST change on 25.10.2026 01:00 UTC, +2 after it.
    fn kyiv(ms: i64) -> i32 {
        if ms < crate::sys::days_from_civil(2026, 10, 25) * 86_400_000 + 3_600_000 {
            180
        } else {
            120
        }
    }
    let ten_utc = |m: u32, d: u32| Some(crate::sys::days_from_civil(2026, m, d) * 86_400_000 + 10 * 3_600_000);
    let msgs = [
        ChatMessage {
            speaker: Speaker::User,
            text: "summer".into(),
            at_ms: ten_utc(10, 20),
        },
        ChatMessage {
            speaker: Speaker::Assistant,
            text: "winter".into(),
            at_ms: ten_utc(11, 20),
        },
    ];
    let lines = chat::chat_lines(ChatView::Messages(&msgs), 60, 10, true, Zone(kyiv)).join("\n");
    assert!(lines.contains("20.10 13:00"), "{lines}");
    assert!(lines.contains("20.11 12:00"), "{lines}");
}

#[test]
fn a_chat_that_fits_starts_under_the_rule_and_a_longer_one_ends_on_the_last_row() {
    let msg = |speaker, text: &str| ChatMessage {
        speaker,
        text: text.into(),
        at_ms: None,
    };
    let few = [msg(Speaker::User, "short question"), msg(Speaker::Assistant, "short answer")];
    let fit = chat::chat_lines(ChatView::Messages(&few), 60, 20, true, Zone(|_| 0));
    assert!(fit[0].contains("you"), "the first message opens the pane: {fit:#?}");
    assert_eq!(fit.len(), 5, "no padding above or below: {fit:#?}");
    let many: Vec<ChatMessage> = (0..12).map(|i| msg(Speaker::Assistant, &format!("answer {i}"))).collect();
    let over = chat::chat_lines(ChatView::Messages(&many), 60, 8, true, Zone(|_| 0));
    assert_eq!(over.len(), 8);
    assert!(over[7].contains("answer 11"), "the newest ends on the last row: {over:#?}");
}

#[test]
fn a_wide_pane_wraps_message_text_at_110_and_keeps_the_owners_at_the_right_edge() {
    let long = "word ".repeat(120);
    let msgs = [
        ChatMessage {
            speaker: Speaker::User,
            text: long.clone(),
            at_ms: None,
        },
        ChatMessage {
            speaker: Speaker::Assistant,
            text: long,
            at_ms: None,
        },
    ];
    let lines = chat::chat_lines(ChatView::Messages(&msgs), 230, 60, true, Zone(|_| 0));
    let plain: Vec<String> = lines.iter().map(|l| crate::theme::strip_ctl(l)).collect();
    let text: Vec<&String> = plain.iter().filter(|l| l.contains("word")).collect();
    assert!(text.iter().all(|l| l.trim().len() <= 110), "{plain:#?}");
    let user = text.first().unwrap();
    assert_eq!(
        crate::layout::display_width(user.trim_end()),
        230 - 2,
        "the owner's text ends at the right inset"
    );
    assert!(text.last().unwrap().starts_with("  word"), "Claude's stays left");
}

#[test]
fn the_list_takes_at_least_half_and_grows_with_no_jump() {
    let mut last = sessions_width(119);
    for fw in 120..=320 {
        let lw = sessions_width(fw);
        assert!(lw * 2 >= fw, "{fw}: the list gets {lw}, under half");
        assert!(fw - lw <= CHAT_PANEL, "{fw}: the chat never passes its 118");
        assert!(lw - last <= 1, "{fw}: the list jumps from {last} to {lw}");
        last = lw;
    }
}
