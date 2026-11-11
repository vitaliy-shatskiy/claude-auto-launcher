use super::*;
use crate::frame::{CellSpan, ListMap, RowHit};

fn span(start: i32, end: i32, line: usize) -> FooterSpan {
    FooterSpan {
        start,
        end,
        key_start: 0,
        key_end: 0,
        key: "Enter".into(),
        ch: None,
        line,
    }
}

#[test]
fn a_console_without_armed_input_ends_as_lost_not_as_exhausted_keys() {
    let logger = Logger {
        root: std::path::PathBuf::new(),
        run_id: String::new(),
        enabled: false,
        timings: crate::timing::Timings::default(),
    };
    let mut t = ConsoleTerm::open(false, None, logger);
    let size = t.size();
    assert_eq!(t.wait(size), Err(UiStop::ConsoleLost));
}

#[test]
fn frame_write_homes_erases_each_line_and_below() {
    assert_eq!(frame_bytes(&["a".into(), "b".into()]), "\x1b[Ha\x1b[K\nb\x1b[K\x1b[J");
}

#[test]
fn a_full_height_frame_feeds_no_line_on_the_bottom_row() {
    // A line feed on the bottom row scrolls the buffer: the top row is lost on every paint.
    let rows: Vec<String> = (0..24).map(|i| format!("row {i}")).collect();
    assert_eq!(frame_bytes(&rows).matches('\n').count(), 23);
}

#[test]
fn hit_footer_then_cells_then_rows() {
    let map = RowMap {
        rows: vec![RowHit {
            index: 1,
            name: "Remote".into(),
            y: 4,
            cells: vec![CellSpan {
                start: 15,
                end: 20,
                value: "on".into(),
            }],
        }],
        footer_y: Some(10),
        footer: vec![span(5, 9, 0), span(2, 6, 1)],
        ..RowMap::default()
    };
    assert!(matches!(hit_at(Some(&map), 6, 10, 0), Hit::Footer { index: 0, .. }));
    assert!(
        matches!(hit_at(Some(&map), 3, 11, 0), Hit::Footer { index: 1, .. }),
        "the wrapped line"
    );
    assert_eq!(
        hit_at(Some(&map), 16, 4, 0),
        Hit::Cell {
            row: 1,
            name: "Remote".into(),
            value: "on".into()
        }
    );
    assert_eq!(
        hit_at(Some(&map), 2, 4, 0),
        Hit::LaunchRow {
            row: 1,
            name: "Remote".into()
        }
    );
    assert_eq!(hit_at(Some(&map), 2, 5, 0), Hit::None);
    assert_eq!(
        hit_at(Some(&map), 16, 104, 100),
        hit_at(Some(&map), 16, 4, 0),
        "window top is subtracted"
    );
}

#[test]
fn list_rows_by_arithmetic_or_explicit_ys() {
    let map = RowMap {
        list: Some(ListMap {
            first_row_y: 3,
            row_count: 4,
            start: 10,
            row_ys: Vec::new(),
        }),
        ..RowMap::default()
    };
    assert_eq!(hit_at(Some(&map), 0, 5, 0), Hit::ListRow(12));
    assert_eq!(hit_at(Some(&map), 0, 7, 0), Hit::None);
    let gaps = RowMap {
        list: Some(ListMap {
            row_ys: vec![3, 5],
            start: 0,
            ..ListMap::default()
        }),
        ..RowMap::default()
    };
    assert_eq!(hit_at(Some(&gaps), 0, 4, 0), Hit::None, "a click on a gap is no row");
    assert_eq!(hit_at(Some(&gaps), 0, 5, 0), Hit::ListRow(1));
}

#[test]
fn preview_keys_and_size() {
    let k = parse_preview_keys("DownArrow,RightArrow,r,Enter").unwrap();
    assert_eq!(k.len(), 4);
    assert!(parse_preview_keys("Bogus").is_err());
    assert_eq!(parse_preview_size(Some("120x30")), (120, 30));
    assert_eq!(parse_preview_size(None), (78, 24));
}

fn console_mouse(buttons: u32, flags: u32) -> Peeked {
    Peeked::Mouse(crate::input::mouse_from_record(5, 2, buttons, flags))
}

fn typed(s: &str) -> Vec<Peeked> {
    s.chars().map(Peeked::KeyDown).collect()
}

#[test]
fn a_waiting_console_button_press_is_a_press() {
    assert!(press_waiting(&[console_mouse(0, 0x1), console_mouse(1, 0)]));
    assert!(press_waiting(&[console_mouse(1, 0x2)]), "the double-click record");
}

#[test]
fn motion_wheel_release_and_keys_are_no_press() {
    assert!(!press_waiting(&[
        console_mouse(0, 0x1),
        console_mouse(1, 0x1),
        console_mouse(0, 0),
        console_mouse(0x0078_0000, 0x4),
        Peeked::KeyDown('s'),
        Peeked::Other,
    ]));
    assert!(!press_waiting(&[]));
}

#[test]
fn a_text_mouse_press_counts_and_its_motion_does_not() {
    assert!(
        press_waiting(&typed("\x1b[<35;6;3M\x1b[<0;6;3M")),
        "SGR press after a motion report"
    );
    assert!(
        !press_waiting(&typed("\x1b[<35;6;3M\x1b[<32;7;3M\x1b[<0;6;3m")),
        "SGR motion and release"
    );
    assert!(!press_waiting(&typed("\x1b[<64;6;3M")), "SGR wheel");
    assert!(press_waiting(&typed("\x1b[M &#")), "X10 press");
    assert!(!press_waiting(&typed("\x1b[M#&#")), "X10 release");
    assert!(!press_waiting(&typed("sw\x1b[A")), "keys and a cursor key");
}
