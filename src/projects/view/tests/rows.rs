//! Project frame: separators, the detail line, hover bands, widths and ages.

use super::*;

#[test]
fn task9_separators_detail_line_and_dim_paths() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha"), proj("B", r"C:\w\beta", "beta")];
    let f = project_frame(&view(&ps, 0, r"C:\Users\sample\here", 80, 24));
    let body = plain(&f);
    let l = f.map.list.clone().unwrap();
    let ys: Vec<usize> = l.row_ys.iter().map(|y| usize::try_from(*y).unwrap()).collect();
    let blank = |s: &str| s.chars().skip(1).take(s.chars().count() - 2).all(|c| c == ' ');
    assert!(body[ys[0]].contains("current directory") && blank(&body[ys[0] + 1]));
    assert!(body[ys[1]].contains("alpha"));
    assert!(body[ys[3]].contains("enter a path") && blank(&body[ys[3] - 1]));
    assert_eq!(ys.len(), 4);
    assert_eq!(l.row_count, 4);
    let ay = usize::try_from(f.map.action.unwrap().y).unwrap();
    assert_eq!(ys[3], ay - 2);
    assert!(body[ay - 1].contains(r"C:\Users\sample\here") && body[ay].contains("action"));
    assert_eq!(f.lines.len(), 4 + 2 + 1 + 1 + 2 + f.map.footer_lines, "the line accounting");
    let c = project_frame(&ProjectView {
        style: Style { color: true, ascii: false },
        ..view(&ps, 1, r"C:\x", 80, 24)
    });
    let cy = usize::try_from(c.map.action.clone().unwrap().y).unwrap();
    assert!(c.lines[cy - 1].contains(&format!("{}{}", theme::DIM, r"C:\w\alpha")));
    assert!(!c.lines.iter().any(|x| x.contains(theme::DIM_OPEN) || x.contains(theme::DIM_CLOSE)));
    let last = usize::try_from(*c.map.list.unwrap().row_ys.last().unwrap()).unwrap();
    assert!(c.lines[last].contains(&format!("{}+", theme::MAGENTA)));
    let a = project_frame(&ProjectView {
        style: Style { color: true, ascii: true },
        ..view(&ps, 0, r"C:\x", 80, 24)
    });
    assert!(!a.lines[0].contains('\x1b'));
    let fy = usize::try_from(a.map.footer_y.unwrap()).unwrap();
    assert!(!a.lines[fy - 1].contains('\x1b'), "nor the bottom border");
    let e = project_frame(&ProjectView {
        filter: "zzz-matches-no-project",
        ..view(&ps, 0, r"C:\cwd", 80, 24)
    });
    let eys = e.map.list.unwrap().row_ys;
    assert_eq!(eys.len(), 2);
    assert_eq!(eys[1] - eys[0] - 1, 1, "exactly one blank line between the pinned rows");
}

#[test]
fn a_project_named_like_the_bullet_is_not_painted_as_one() {
    let ps = vec![proj("P", r"C:\w\plus", "+ plus project")];
    for ascii in [false, true] {
        let f = project_frame(&ProjectView {
            style: Style { color: true, ascii },
            ..view(&ps, 0, r"C:\x", 80, 24)
        });
        let ys = f.map.list.unwrap().row_ys;
        let at = |i: usize| f.lines[usize::try_from(ys[i]).unwrap()].clone();
        assert!(!at(1).contains(theme::MAGENTA), "ascii={ascii}");
        assert!(at(ys.len() - 1).contains(&format!("{}+", theme::MAGENTA)), "ascii={ascii}");
    }
}

#[test]
fn cs218_one_height_cursor_in_window_no_blank_above_action() {
    let ps = many(40, "project-name");
    for (w, h, detail) in [(50, 20, 1), (120, 30, 0)] {
        let all = project_rows(&ps, "", r"C:\x");
        let mut height = None;
        let mut prev: Option<i32> = None;
        for gi in 0..all.len() {
            let f = project_frame(&view(&ps, gi, r"C:\x", w, h));
            let l = f.map.list.clone().unwrap();
            assert_eq!(*height.get_or_insert(f.lines.len()), f.lines.len(), "{w}x{h} {gi}: height");
            let next = usize::try_from(l.start + l.row_count).unwrap();
            let sep = all.get(next).is_some_and(|r| r.kind == RowKind::Path);
            if !sep {
                assert_eq!(
                    l.row_ys.last().unwrap() + 1 + detail,
                    f.map.action.clone().unwrap().y,
                    "{w}x{h} {gi}: blank above action"
                );
            }
            let gi32 = i32::try_from(gi).unwrap();
            assert!(
                gi32 >= l.start && gi32 < l.start + l.row_count,
                "{w}x{h} {gi}: cursor off the window"
            );
            if let Some(p) = prev {
                assert!((l.start - p).abs() <= 1, "{w}x{h} {gi}: jump");
            }
            prev = Some(l.start);
            assert!(f.lines.len() < h, "{w}x{h} {gi}: headroom row");
        }
    }
    let top = project_frame(&view(&ps, 0, r"C:\x", 50, 20)).map.list.unwrap().row_count;
    let mid = project_frame(&view(&ps, 20, r"C:\x", 50, 20)).map.list.unwrap().row_count;
    let bottom = project_frame(&view(&ps, 41, r"C:\x", 50, 20)).map.list.unwrap().row_count;
    assert_eq!(top + 1, mid);
    assert_eq!(top, bottom);
}

#[test]
fn hover_bands_the_row_and_the_value() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha"), proj("B", r"C:\w\beta", "beta")];
    let color = Style { color: true, ascii: false };
    let f = project_frame(&ProjectView {
        hover_row: 1,
        style: color,
        ..view(&ps, 0, r"C:\somewhere", 120, 24)
    });
    let ys = f.map.list.clone().unwrap().row_ys;
    assert!(f.lines[usize::try_from(ys[1]).unwrap()].contains(theme::BUTTON_BG));
    assert!(!f.lines[usize::try_from(ys[2]).unwrap()].contains(theme::BUTTON_BG));
    let v = project_frame(&ProjectView {
        hover_value: "resume",
        style: color,
        ..view(&ps, 0, r"C:\somewhere", 120, 24)
    });
    let ay = usize::try_from(v.map.action.unwrap().y).unwrap();
    assert!(v.lines[ay].contains(&format!("{}\u{25CB} resume", theme::BUTTON_BG)));
}

#[test]
fn every_frame_fits_below_the_last_column() {
    let ps = many(40, "a-rather-long-project-name-for-the-width-check");
    for w in [50, 60, 80, 99, 100, 120, 200] {
        for (color, ascii) in [(false, false), (true, false), (true, true)] {
            let f = project_frame(&ProjectView {
                filter: "project",
                notice: "path not found",
                action: "resume",
                hover_row: 1,
                hover_value: "resume",
                style: Style { color, ascii },
                ..view(&ps, 0, r"C:\Users\sample", w, 24)
            });
            for l in &f.lines {
                assert!(display_width(&theme::remove_ansi_color(l)) < w, "{w} {l}");
            }
        }
    }
}

#[test]
fn relative_age_rounds_like_n0() {
    assert_eq!(relative_age(NOW - 30_000, NOW), "now");
    assert_eq!(relative_age(NOW - 4 * MIN, NOW), "4 min");
    assert_eq!(relative_age(NOW - 8 * 60 * MIN, NOW), "8 h");
    assert_eq!(relative_age(NOW - 1500 * 1440 * MIN, NOW), "1,500 d");
    assert_eq!(clean_text("a\u{1b}]0;x\u{7}  b\u{200B}"), "a ]0;x b");
}
