#![expect(clippy::many_single_char_names, reason = "short fixture names")]

use super::*;
use crate::projects::select::tests::proj;
use std::path::Path;

const NOW: i64 = 1_789_560_000_000;
const MIN: i64 = 60_000;

fn golden_projects() -> Vec<Project> {
    let p = |name: &str, path: &str, wt: bool, ago: i64| Project {
        worktree: wt.then(|| "feature".to_owned()),
        last_activity_ms: Some(NOW - ago),
        ..proj(name, path, name)
    };
    vec![
        p("alpha", r"C:\Users\sample\Desktop\Projects\alpha", false, 4 * MIN),
        p(
            "beta-service",
            r"C:\Users\sample\Desktop\Projects\workspace\beta-service",
            false,
            8 * 60 * MIN,
        ),
        p(
            "gamma",
            r"C:\Users\sample\Desktop\Projects\gamma.worktrees\feature",
            true,
            3 * 1440 * MIN,
        ),
        p("delta", r"D:\src\delta", false, 23 * 1440 * MIN),
    ]
}

fn view<'a>(ps: &'a [Project], index: usize, cwd: &'a str, w: usize, h: usize) -> ProjectView<'a> {
    ProjectView {
        projects: ps,
        marked: &[],
        dimmed: &[],
        pinned: &[],
        index,
        filter: "",
        typing: false,
        show_hidden: false,
        cwd,
        width: w,
        height: h,
        now_ms: NOW,
        style: Style {
            color: false,
            ascii: false,
        },
        hover: -1,
        notice: "",
        action: "new",
        hover_row: -1,
        hover_value: "",
        detail: None,
        home: None,
    }
}

fn golden(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wide-frames")
        .join(format!("{name}.txt"));
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
}

/// Compares with the golden; `CLAUDE_AUTO_BLESS` set rewrites it instead (review the diff).
fn assert_golden(name: &str, actual: &str) {
    if std::env::var_os("CLAUDE_AUTO_BLESS").is_some() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wide-frames")
            .join(format!("{name}.txt"));
        std::fs::write(p, actual).unwrap();
        return;
    }
    assert_eq!(actual, golden(name), "{name}");
}

fn text(f: &Frame) -> String {
    f.lines.join("\n")
}

fn plain(f: &Frame) -> Vec<String> {
    f.lines.iter().map(|l| theme::remove_ansi_color(l)).collect()
}

/// The cell column `needle` starts at in `line`.
fn col(line: &str, needle: &str) -> usize {
    display_width(line.split_once(needle).unwrap().0)
}

#[test]
fn wide_rows_keep_name_path_and_age_in_compact_columns() {
    let ps = golden_projects();
    let f = project_frame(&view(&ps, 1, r"C:\Users\sample\here", 200, 60));
    let lw = list_panel_width(&f);
    let lines = plain(&f);
    // The list panel's own columns.
    let left = |l: &String| l.chars().take(lw).collect::<String>();
    let row = |name: &str| left(lines.iter().find(|l| left(l).contains(&format!(" {name} "))).unwrap());
    let path_col = col(&row("gamma"), r"C:\Users");
    assert_eq!(col(&row("alpha"), r"C:\Users"), path_col, "paths share one column");
    assert_eq!(col(&row("delta"), r"D:\src"), path_col);
    assert_eq!(col(&row("directory"), r"C:\Users"), path_col, "the cwd row too");
    assert_eq!(
        path_col,
        4 + display_width("current directory") + 2,
        "two cells after the longest name"
    );
    let age_end = |l: &str, age: &str| col(l, age) + display_width(age);
    let end = lw - 3;
    assert_eq!(age_end(&row("alpha"), "4 min"), end, "ages right-aligned 2 cells from the border");
    assert_eq!(age_end(&row("delta"), "23 d"), end);
}

#[test]
fn a_long_name_is_capped_at_forty_percent_and_the_path_cut_to_fit() {
    let mut ps = golden_projects();
    ps[0].name = "n".repeat(150);
    let f = project_frame(&view(&ps, 1, r"C:\Users\sample\here", 120, 30));
    let inner = list_panel_width(&f) - 4;
    let lines = plain(&f);
    let alpha = lines.iter().skip(1).find(|l| l.contains("nnn")).unwrap();
    assert_eq!(col(alpha, "C:"), 4 + inner * 2 / 5 + 2, "{alpha}");
    assert!(alpha.contains('\u{2026}'), "the name ends in an ellipsis: {alpha}");
    assert!(lines.iter().all(|l| display_width(l) <= 119));
}

#[test]
fn golden_project_100x30() {
    let ps = golden_projects();
    let f = project_frame(&view(&ps, 1, r"C:\Users\sample\here", 100, 30));
    assert_golden("project-100x30", &text(&f));
}

#[test]
fn golden_project_120x30_hover_color() {
    let ps = golden_projects();
    let f = project_frame(&ProjectView {
        action: "continue",
        hover_row: 1,
        hover_value: "resume",
        hover: 0,
        style: Style { color: true, ascii: false },
        ..view(&ps, 2, r"C:\Users\sample\here", 120, 30)
    });
    assert_golden("project-120x30-hover-color", &text(&f));
}

/// The selected project's context as the detail worker builds it: a branch, five sessions, the
/// newest running.
fn sample_detail(path: &str) -> Detail {
    let s = |title: &str, ago: i64, live: bool| crate::projects::detail::SessionLine {
        title: title.into(),
        modified_ms: NOW - ago,
        live,
        session_id: format!("id-{title}"),
    };
    Detail {
        key: project_key(path),
        branch: Some("main".into()),
        count: 18,
        sessions: vec![
            s("design the cockpit panels", 4 * MIN, true),
            s("fix the invoice export paging", 3 * 60 * MIN, false),
            s("triage the sync job alerts", 26 * 60 * MIN, false),
            s("compare two apk resource trees", 3 * 1440 * MIN, false),
            s("restart loop on the VM", 9 * 1440 * MIN, false),
        ],
    }
}

#[test]
fn goldens_project_280x72_and_130x40_with_the_selected_context() {
    let ps = golden_projects();
    let d = sample_detail(&ps[0].path);
    for (w, h) in [(280, 72), (130, 40)] {
        let f = project_frame(&ProjectView {
            detail: Some(&d),
            ..view(&ps, 1, r"C:\Users\sample\here", w, h)
        });
        assert_golden(&format!("project-{w}x{h}-context"), &text(&f));
        assert_eq!(f.lines.len(), h - 1);
        let panels = usize::try_from(f.map.footer_y.unwrap()).unwrap();
        assert!(
            f.lines.iter().take(panels).all(|l| display_width(l) == w - 1),
            "{w}x{h}: the panels span the frame"
        );
    }
}

#[test]
fn the_list_takes_what_its_rows_need_and_the_context_the_rest() {
    let ps = golden_projects();
    let d = sample_detail(&ps[0].path);
    let lines = plain(&project_frame(&ProjectView {
        detail: Some(&d),
        ..view(&ps, 1, r"C:\Users\sample\here", 280, 72)
    }));
    let corner = lines[0].chars().position(|c| c == '\u{256E}').unwrap() + 1;
    let longest = r"C:\Users\sample\Desktop\Projects\gamma.worktrees\feature";
    assert!(lines.iter().any(|l| l.contains(longest)), "the full path, never cut under the cap");
    assert_eq!(
        corner,
        279 - crate::cockpit::FACTS_WIDTH,
        "the context keeps its width, the list the rest"
    );
    let right = |l: &String| l.chars().skip(corner).collect::<String>();
    assert!(right(&lines[0]).contains("selected \u{b7} alpha"));
    for want in [
        "branch    main",
        "sessions  18",
        "last      4 min  design the cockpit panels",
        "  3 h  fix the invoice export paging",
    ] {
        assert!(lines.iter().any(|l| right(l).contains(want)), "{want}");
    }
    let action = lines.iter().position(|l| l.contains("action") && l.contains("[new]")).unwrap();
    assert!(
        lines[action].chars().position(|c| c == '[').unwrap() < corner,
        "the action line sits in the list panel"
    );
    assert_eq!(action, 68, "on the list panel's last interior row");
}

#[test]
fn no_sessions_and_loading_say_so() {
    let ps = golden_projects();
    let empty = Detail {
        key: project_key(&ps[0].path),
        ..Detail::default()
    };
    let none = text(&project_frame(&ProjectView {
        detail: Some(&empty),
        ..view(&ps, 1, r"C:\Users\sample\here", 130, 40)
    }));
    assert!(none.contains("no sessions yet"));
    let loading = text(&project_frame(&view(&ps, 1, r"C:\Users\sample\here", 130, 40)));
    assert!(loading.contains("loading\u{2026}") && !loading.contains("no sessions yet"));
}

#[test]
fn golden_project_200x60_fills_the_terminal() {
    let ps = golden_projects();
    let f = project_frame(&view(&ps, 1, r"C:\Users\sample\here", 200, 60));
    assert_golden("project-200x60", &text(&f));
    assert_eq!(f.lines.len(), 59, "AC27: every row but the headroom one");
    assert_eq!(display_width(&f.lines[0]), 199);
}

#[test]
fn golden_project_400x150_fills_the_terminal_from_the_top_left() {
    let ps = golden_projects();
    let area = frame::Area::of((400, 150));
    let lines = project_frame(&view(&ps, 1, r"C:\Users\sample\here", area.width, area.height)).lines;
    assert_golden("project-400x150", &lines.join("\n"));
    assert_eq!(lines.len(), 149);
    assert!(lines[0].starts_with('\u{256D}'), "the box starts at column 0: {}", lines[0]);
    assert_eq!(display_width(&lines[0]), 399);
}

#[test]
fn the_footer_names_the_action_switch_keys_and_the_action_row_does_not() {
    let ps = golden_projects();
    let f = project_frame(&view(&ps, 1, r"C:\Users\sample\here", 100, 30));
    let y = usize::try_from(f.map.action.as_ref().unwrap().y).unwrap();
    assert!(!f.lines[y].contains("a/d"), "{}", f.lines[y]);
    let fy = usize::try_from(f.map.footer_y.unwrap()).unwrap();
    assert!(f.lines[fy].contains("a/d \u{2190}/\u{2192} action"), "{}", f.lines[fy]);
}

fn many(n: usize, prefix: &str) -> Vec<Project> {
    (1..=n)
        .map(|i| Project {
            last_activity_ms: Some(NOW - i64::try_from(i).unwrap() * MIN),
            ..proj(&format!("S{i}"), &format!(r"C:\w\{prefix}{i}"), &format!("{prefix}-{i}"))
        })
        .collect()
}

#[test]
fn task6_rows_map_footer_and_cwd_path() {
    let ps = vec![
        Project {
            last_activity_ms: Some(NOW - 2 * MIN),
            ..proj("A", r"C:\w\alpha", "alpha")
        },
        Project {
            last_activity_ms: Some(NOW - 1440 * MIN),
            ..proj("B", r"C:\w\beta", "beta")
        },
    ];
    let f = project_frame(&view(&ps, 0, r"C:\somewhere", 78, 24));
    let t = text(&f);
    for s in [
        "alpha",
        "current directory",
        "enter a path",
        "continue",
        "worktree",
        r"C:\somewhere",
    ] {
        assert!(t.contains(s), "{s}");
    }
    let l = f.map.list.clone().unwrap();
    assert_eq!((l.row_count, l.first_row_y, l.start), (4, 1, 0));
    assert_eq!(
        f.lines
            .iter()
            .filter(|x| ["alpha", "beta", "current directory", "enter a path"].iter().any(|s| x.contains(s)))
            .count(),
        4
    );
    let filtered = project_frame(&ProjectView {
        filter: "alpha",
        ..view(&ps, 0, r"C:\somewhere", 78, 24)
    });
    assert_eq!(filtered.map.list.clone().unwrap().row_count, 3);
    assert!(text(&filtered).contains("1 known") && text(&filtered).contains("filter: alpha"));
    let small = project_frame(&view(&ps, 0, r"C:\somewhere", 50, MIN_HEIGHT - 1));
    assert!(small.lines.iter().any(|x| x.contains(&format!("need {MIN_WIDTH}x{MIN_HEIGHT}"))));
    assert!(text(&project_frame(&view(&[], 0, r"C:\somewhere", 78, 24))).contains("current directory"));
}

#[test]
fn a_scrolled_list_maps_its_real_start() {
    let ps = many(30, "scrollproj");
    let f = project_frame(&view(&ps, 29, r"C:\somewhere", 78, 24));
    let l = f.map.list.unwrap();
    // The footer is one row at every width: the list keeps the rows above it.
    assert_eq!((l.first_row_y, l.row_count, l.start), (1, 17, 15));
}

#[test]
fn min_height_frame_is_real_and_leaves_headroom() {
    let ps = many(30, "project");
    let f = project_frame(&view(&ps, 0, r"C:\somewhere", 50, MIN_HEIGHT));
    assert!(text(&f).contains("known"));
    assert!(f.lines.len() < MIN_HEIGHT);
    let big = many(40, "project-name");
    let fit = project_frame(&ProjectView {
        action: "worktree",
        ..view(&big, 5, r"C:\x", 50, MIN_HEIGHT)
    });
    assert!(fit.lines.len() < MIN_HEIGHT && text(&fit).contains("[worktree]"));
    let tall = project_frame(&ProjectView {
        action: "worktree",
        ..view(&big, 5, r"C:\x", 50, 200)
    });
    assert!(tall.lines.len() > fit.lines.len());
}

#[test]
fn long_path_and_long_name_keep_the_age() {
    let deep = Project {
        last_activity_ms: Some(NOW - 5 * MIN),
        ..proj("L", &format!(r"C:\{}end", r"deepfolder\".repeat(30)), "longname")
    };
    assert!(text(&project_frame(&view(&[deep], 0, r"C:\x", 78, 24))).contains("5 min"));
    let long = Project {
        last_activity_ms: Some(NOW - 5 * MIN),
        ..proj("N", r"C:\p", &"n".repeat(60))
    };
    assert!(text(&project_frame(&view(&[long], 0, r"C:\x", 50, 24))).contains("5 min"));
}

#[test]
fn narrow_detail_lines_middle_cut_and_keep_the_leaf() {
    let root = r"C:\Users\sample\Desktop\Projects\workspace\services";
    let ps = vec![
        Project {
            last_activity_ms: Some(NOW - 5 * MIN),
            ..proj("d4a", &format!(r"{root}\alpha"), "alpha")
        },
        Project {
            last_activity_ms: Some(NOW - 5 * MIN),
            ..proj("d4b", &format!(r"{root}\beta"), "alpha")
        },
    ];
    let line = |i: usize| {
        let f = project_frame(&view(&ps, i, r"C:\x", 50, 20));
        let y = usize::try_from(f.map.action.clone().unwrap().y).unwrap();
        plain(&f)[y - 1].clone()
    };
    let (a, b) = (line(1), line(2));
    assert!(a != b && a.contains('\u{2026}'));
    assert!(a.contains(r"\alpha") && b.contains(r"\beta") && a.contains(r"C:\") && b.contains(r"C:\"));
}

#[test]
fn action_field_radio_cells_and_compact_form() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha"), proj("B", r"C:\w\beta", "beta")];
    let f = project_frame(&ProjectView {
        action: "resume",
        ..view(&ps, 0, r"C:\cwd", 80, 24)
    });
    let a = f.map.action.clone().unwrap();
    let line: Vec<char> = f.lines[usize::try_from(a.y).unwrap()].chars().collect();
    assert!(line.iter().collect::<String>().contains("action  \u{25CB} new"));
    assert_eq!(a.cells.len(), 4);
    let wt = &a.cells[3];
    assert_eq!(wt.value, "worktree");
    let s: String = line[usize::try_from(wt.start).unwrap()..=usize::try_from(wt.end).unwrap()]
        .iter()
        .collect();
    assert_eq!(s, "\u{25CB} worktree");
    assert!(text(&f).contains(" \u{276F} current directory"));
    let n = project_frame(&ProjectView {
        action: "resume",
        ..view(&ps, 0, r"C:\cwd", 50, 24)
    });
    let na = n.map.action.unwrap();
    let nl = &n.lines[usize::try_from(na.y).unwrap()];
    assert!(nl.contains("[resume]") && !nl.contains('\u{25CF}'));
    assert_eq!(na.cells.len(), 4);
    let case = project_frame(&ProjectView {
        action: "RESUME",
        ..view(&ps, 0, r"C:\cwd", 80, 24)
    });
    assert!(text(&case).contains("\u{25CF} [resume]"));
    assert_eq!(step_project_action("RESUME", 1), "worktree");
    assert_eq!(step_project_action("ReSuMe", 0), "resume");
    assert_eq!(step_project_action("new", -1), "worktree");
    assert_eq!(step_project_action("not-an-action", 0), "new");
    let f80 = project_frame(&view(&ps, 0, r"C:\cwd", 80, 24));
    assert!(
        text(&f80).contains("a/d \u{2190}/\u{2192} action") && text(&f80).contains("[esc] back"),
        "a footer too narrow for every button keeps the move and switch hints"
    );
    assert_eq!(f80.map.footer_lines, 1, "the footer never wraps");
}

/// Item 5: w/s, a/d, enter, the filter and esc survive every width from 78 up in one footer row;
/// the other buttons drop, the last listed first, and all of them show where they fit.
#[test]
fn the_move_and_action_hints_survive_78_to_120_columns() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha")];
    for w in [78, 100, 120, 160] {
        let f = project_frame(&view(&ps, 0, r"C:\cwd", w, 24));
        let fy = usize::try_from(f.map.footer_y.unwrap()).unwrap();
        assert_eq!(f.map.footer_lines, 1, "{w}: one footer row");
        let footer = &f.lines[fy];
        for hint in [
            "w/s move",
            "a/d \u{2190}/\u{2192} action",
            "[enter] run",
            "[/] filter",
            "[esc] back",
        ] {
            assert!(footer.contains(hint), "{w}: {hint} in\n{footer}");
        }
        assert!(f.lines.iter().all(|l| display_width(l) < w), "{w}: fits");
    }
    let wide = project_frame(&view(&ps, 0, r"C:\cwd", 200, 24));
    let footer = &wide.lines[usize::try_from(wide.map.footer_y.unwrap()).unwrap()];
    for hint in ["[c] continue", "[p] pin", "[h] hide", "[v] hidden"] {
        assert!(footer.contains(hint), "200: {hint} in\n{footer}");
    }
}

/// Item 4: the footer names the keys of the state it is in.
#[test]
fn the_footer_follows_filter_kept_filter_and_hidden_view() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha")];
    let footer = |v: ProjectView<'_>| {
        let f = project_frame(&v);
        let fy = usize::try_from(f.map.footer_y.unwrap()).unwrap();
        (f.lines[fy..].join("\n"), f.map.footer)
    };
    let (open, spans) = footer(ProjectView {
        filter: "al",
        typing: true,
        ..view(&ps, 0, r"C:\cwd", 78, 24)
    });
    assert!(
        open.contains("[enter] run") && open.contains("[esc] clear") && open.contains("up/down move"),
        "{open}"
    );
    assert!(!open.contains("[c]") && !open.contains("[/]"), "{open}");
    assert!(
        spans.iter().all(|s| s.ch.is_none()),
        "no button of an open filter types a character"
    );
    let (kept, _) = footer(ProjectView {
        filter: "al",
        ..view(&ps, 0, r"C:\cwd", 78, 24)
    });
    assert!(kept.contains("[esc] clear") && kept.contains("[/] filter"), "{kept}");
    let (hidden, _) = footer(ProjectView {
        show_hidden: true,
        ..view(&ps, 0, r"C:\cwd", 200, 24)
    });
    assert!(hidden.contains("[v] back") && !hidden.contains("[v] hidden"), "{hidden}");
}

/// Item 6: the matched text is marked in the name and the path, never in the mark or the age.
#[test]
fn filter_matches_are_highlighted_in_name_and_path() {
    let ps = vec![Project {
        last_activity_ms: Some(NOW - 4 * MIN),
        ..proj("A", r"C:\w\alpha-min", "alpha-min")
    }];
    let f = project_frame(&ProjectView {
        filter: "MIN al",
        style: Style { color: true, ascii: false },
        ..view(&ps, 1, r"C:\cwd", 120, 24)
    });
    let y = usize::try_from(f.map.list.clone().unwrap().row_ys[1]).unwrap();
    let row = &f.lines[y];
    assert_eq!(
        row.matches(MATCH_ON).count(),
        4,
        "al and min in the name, al and min in the path: {row:?}"
    );
    assert!(!row.contains(&format!("4 {MATCH_ON}")), "not the age: {row:?}");
    let plain_frame = project_frame(&ProjectView {
        filter: "al",
        ..view(&ps, 1, r"C:\cwd", 120, 24)
    });
    assert!(!text(&plain_frame).contains(['\u{6}', '\u{e}']), "colour off leaves no marker");
    assert_eq!(mark_matches("   alpha", &["al".into()], 3, usize::MAX), "   \u{6}al\u{e}pha");
}

#[test]
fn footer_lights_the_selected_action_and_the_hover() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha")];
    let lit = format!("{}{} r ", theme::ACCENT_BG, theme::ACCENT_FG);
    let color = Style { color: true, ascii: false };
    let r = project_frame(&ProjectView {
        action: "resume",
        style: color,
        ..view(&ps, 0, r"C:\cwd", 200, 24)
    });
    assert!(text(&r).contains(&lit));
    let n = project_frame(&ProjectView {
        style: color,
        ..view(&ps, 0, r"C:\cwd", 200, 24)
    });
    assert!(
        !text(&n).contains(&format!("{}{}", theme::ACCENT_BG, theme::ACCENT_FG)),
        "new lights nothing"
    );
    let c_index = i32::try_from(n.map.footer.iter().position(|s| s.ch == Some('c')).unwrap()).unwrap();
    let h = project_frame(&ProjectView {
        style: color,
        hover: c_index,
        ..view(&ps, 0, r"C:\cwd", 200, 24)
    });
    let fy = usize::try_from(h.map.footer_y.unwrap()).unwrap();
    assert!(h.lines[fy].contains(theme::ACCENT_BG));
}

#[test]
fn title_shows_typing_and_notice() {
    let ps = vec![proj("A", r"C:\w\alpha", "alpha")];
    let t = project_frame(&ProjectView {
        filter: "al",
        typing: true,
        ..view(&ps, 0, r"C:\cwd", 100, 24)
    });
    assert!(text(&t).contains("filter: al_"));
    let n = project_frame(&ProjectView {
        notice: "path not found",
        ..view(&ps, 0, r"C:\cwd", 100, 24)
    });
    assert!(text(&n).contains("path not found"));
    assert!(!text(&project_frame(&view(&ps, 0, r"C:\cwd", 100, 24))).contains("path not found"));
}

#[test]
fn cockpit_goldens_at_130x40_80x24_and_for_a_new_user() {
    let ps = golden_projects();
    let pinned = [project_key(&ps[1].path)];
    let wide = project_frame(&ProjectView {
        pinned: &pinned,
        ..view(&ps, 1, r"C:\Users\sample\here", 130, 40)
    });
    assert_golden("project-130x40", &text(&wide));
    assert!(
        wide.lines[0].contains("pinned 1") && wide.lines[0].contains("worktrees 1"),
        "{}",
        wide.lines[0]
    );
    assert_golden(
        "project-80x24",
        &text(&project_frame(&view(&ps, 1, r"C:\Users\sample\here", 80, 24))),
    );
    assert_golden(
        "project-130x40-s1",
        &text(&project_frame(&view(&[], 0, r"C:\Users\sample\here", 130, 40))),
    );
}

#[test]
fn beside_the_context_the_action_line_closes_the_list_panel_with_its_own_clicks() {
    use crate::ui::{Hit, hit_at};
    let ps = many(12, "p");
    let f = project_frame(&view(&ps, 0, r"C:\cwd", 130, 30));
    let a = f.map.action.clone().unwrap();
    let first = a.cells.first().unwrap();
    let lw = i32::try_from(list_panel_width(&f)).unwrap();
    assert!(a.cells.iter().all(|c| c.end < lw), "the field sits in the list panel");
    let l = f.map.list.clone().unwrap();
    assert!(l.row_ys.iter().all(|y| *y < a.y), "under every list row");
    assert_eq!(hit_at(Some(&f.map), 10, l.row_ys[1], 0), Hit::ListRow(l.start + 1));
    assert_eq!(
        hit_at(Some(&f.map), first.start, a.y, 0),
        Hit::Action {
            value: Some(first.value.clone())
        }
    );
    let cell = |c: &CellSpan| {
        f.lines[usize::try_from(a.y).unwrap()]
            .chars()
            .skip(usize::try_from(c.start).unwrap())
            .take(c.value.len() + 4)
            .collect::<String>()
    };
    assert!(
        a.cells.iter().all(|c| cell(c).contains(&c.value)),
        "the cells point at the drawn values"
    );
}

/// The list panel's outer width: its top border up to the first corner.
fn list_panel_width(f: &Frame) -> usize {
    theme::remove_ansi_color(&f.lines[0]).chars().position(|c| c == '\u{256E}').unwrap() + 1
}

mod pins;
mod rows;
