//! Project frame: pinned rows on top, a pin whose folder is gone drawn dimmed, the `p` button.

use super::*;
use crate::projects::select::pinned_first;

#[test]
fn pins_lead_in_pin_order_whatever_their_recency() {
    let ps = many(4, "p");
    let pins = [r"C:\w\p3".to_owned(), r"c:/W/P1/".to_owned(), r"C:\w\p4".to_owned()];
    let listed = pinned_first(&ps, &pins);
    let f = project_frame(&view(&listed, 0, r"C:\elsewhere", 100, 30));
    let lines = plain(&f);
    let at = |name: &str| {
        lines
            .iter()
            .position(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name}: {lines:#?}"))
    };
    assert!(at("p-3") < at("p-1") && at("p-1") < at("p-4"), "pin order: {lines:#?}");
    assert!(at("p-4") < at("p-2"), "the unpinned newest comes after every pin");
}

#[test]
fn a_missing_pin_is_dimmed_and_marked() {
    let gone = r"C:\w\gone";
    let listed = pinned_first(&many(1, "p"), &[gone.to_owned()]);
    let dimmed = [project_key(gone)];
    let f = project_frame(&ProjectView {
        dimmed: &dimmed,
        style: Style { color: true, ascii: false },
        ..view(&listed, 0, r"C:\elsewhere", 100, 30)
    });
    let row = f.lines.iter().find(|l| l.contains("gone (missing)")).expect("the row is marked");
    assert!(row.contains(&format!("{}gone (missing)", theme::DIM)), "dimmed: {row:?}");
    let live = f.lines.iter().find(|l| l.contains("p-1")).unwrap();
    assert!(!live.contains(&format!("{}p-1", theme::DIM)), "a live row is not dimmed: {live:?}");
}

#[test]
fn pinned_rows_carry_a_dim_pin_mark_before_the_name() {
    let ps = many(3, "p");
    let pins = [r"C:\w\p2".to_owned()];
    let listed = pinned_first(&ps, &pins);
    let keys = [project_key(&pins[0])];
    let pinned = |style| {
        project_frame(&ProjectView {
            pinned: &keys,
            style,
            ..view(&listed, 1, r"C:\elsewhere", 100, 30)
        })
    };
    assert_golden(
        "project-100x30-pinned",
        &text(&pinned(Style {
            color: false,
            ascii: false,
        })),
    );
    let f = pinned(Style { color: true, ascii: false });
    let row = f.lines.iter().find(|l| l.contains("p-2")).expect("the pinned row");
    assert!(row.contains(&format!("{}\u{2191}", theme::DIM)), "a dim mark: {row:?}");
    let other = f.lines.iter().find(|l| l.contains("p-1")).unwrap();
    assert!(!other.contains('\u{2191}'), "an unpinned row has none: {other:?}");
    let ascii = pinned(Style { color: false, ascii: true });
    assert!(plain(&ascii).iter().any(|l| l.contains("^ p-2")), "ASCII: {:#?}", plain(&ascii));
}

#[test]
fn the_footer_offers_p_pin() {
    let ps = many(1, "p");
    let f = project_frame(&view(&ps, 0, r"C:\elsewhere", 100, 30));
    assert!(f.map.footer.iter().any(|s| s.ch == Some('p')), "a p button");
    assert!(plain(&f).iter().any(|l| l.contains("pin")), "named pin");
}
