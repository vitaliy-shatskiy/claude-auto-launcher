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
    assert_ne!(UiStop::ConsoleLost.to_string(), UiStop::KeysExhausted.to_string());
}

#[test]
fn frame_write_homes_erases_each_line_and_below() {
    assert_eq!(frame_bytes(&["a".into(), "b".into()]), "\x1b[Ha\x1b[K\nb\x1b[K\n\x1b[J");
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
