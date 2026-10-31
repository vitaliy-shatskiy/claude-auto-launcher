//! `Get-ProjectFrame` and `Get-ProjectViewport`: where the session runs and what it does there.
//! Pure: every input, the clock included, is a parameter.

use std::sync::LazyLock;

use regex::Regex;

use super::Project;
use super::select::{Row, RowKind, project_count, project_rows};
use crate::frame::{
    self, ActionHit, CellSpan, FooterLight, Frame, Hint, ListMap, MIN_HEIGHT, MIN_WIDTH, RadioSpec, RowMap, Style, TWO_PANE_WIDTH,
    new_hint_footer, new_radio_row,
};
use crate::launch::{LAUNCH_MODES, launch_mode, step_option};
use crate::layout::{display_width, limit_line, limit_path, new_box, viewport};
use crate::list::{ListRow, add_picker_color, new_list_row};
use crate::theme::{self, Glyphs, glyphs};

/// The action field's values: the launch-mode table's names, in its order.
pub(crate) fn project_actions() -> Vec<String> {
    LAUNCH_MODES.iter().map(|m| m.name.to_owned()).collect()
}

/// `Step-ProjectAction`; delta 0 is the canonicaliser (an unknown value is the first).
pub(crate) fn step_project_action(action: &str, delta: i64) -> String {
    step_option(&project_actions(), action, delta)
}

/// Thousands-grouped whole number, the invariant `{0:N0}`.
fn group(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "ages in minutes, hours or days fit easily"
)]
fn rounded(ms: i64, unit_ms: f64) -> i64 {
    (ms as f64 / unit_ms).round() as i64
}

/// `Format-RelativeAge`: now, N min, N h, N d (rounded like `{0:N0}`).
pub(crate) fn relative_age(from_ms: i64, now_ms: i64) -> String {
    let age = now_ms.saturating_sub(from_ms);
    if age < 60_000 {
        "now".into()
    } else if age < 3_600_000 {
        format!("{} min", group(rounded(age, 60_000.0)))
    } else if age < 86_400_000 {
        format!("{} h", group(rounded(age, 3_600_000.0)))
    } else {
        format!("{} d", group(rounded(age, 86_400_000.0)))
    }
}

static CTL_OR_FORMAT: LazyLock<Regex> = LazyLock::new(|| theme::re(r"[\p{Cc}\p{Cf}]"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| theme::re(r"\s+"));

/// `Get-CleanTranscriptText`: control and format characters become spaces, runs of space collapse.
pub(crate) fn clean_text(text: &str) -> String {
    let t = CTL_OR_FORMAT.replace_all(text, " ");
    SPACES.replace_all(&t, " ").trim().to_owned()
}

/// `Get-ProjectViewport`: the window over the rows, separator lines included, centred on the
/// cursor's line. A window never opens on a gap; one closing on a gap ends a line earlier.
/// Returns (first row, visible rows).
pub(crate) fn project_viewport(rows: &[Row], index: usize, lines: usize) -> (usize, usize) {
    let count = rows.len();
    if count == 0 {
        return (0, 0);
    }
    let mut line_row: Vec<Option<usize>> = Vec::new();
    let mut row_line = vec![0usize; count];
    for (i, r) in rows.iter().enumerate() {
        if r.kind == RowKind::Path && i > 0 {
            line_row.push(None);
        }
        row_line[i] = line_row.len();
        line_row.push(Some(i));
        if r.kind == RowKind::Cwd && rows.get(i + 1).is_some_and(|n| n.kind != RowKind::Path) {
            line_row.push(None);
        }
    }
    let index = index.min(count - 1);
    let (start, visible) = viewport(line_row.len(), row_line[index], lines.max(1));
    let mut first = start;
    let mut last = start + visible - 1;
    if line_row.get(first).copied().flatten().is_none() {
        first += 1;
        last = (last + 1).min(line_row.len() - 1);
    }
    while last > first && line_row.get(last).copied().flatten().is_none() {
        last -= 1;
    }
    let a = line_row.get(first).copied().flatten().unwrap_or(0);
    let b = line_row.get(last).copied().flatten().unwrap_or(a);
    (a, b - a + 1)
}

/// Everything one project frame is drawn from.
#[derive(Debug, Clone)]
pub(crate) struct ProjectView<'a> {
    pub projects: &'a [Project],
    pub index: usize,
    pub filter: &'a str,
    pub typing: bool,
    pub cwd: &'a str,
    pub width: usize,
    pub height: usize,
    pub now_ms: i64,
    pub style: Style,
    /// Footer button under the mouse (-1 none).
    pub hover: i32,
    /// A rejected pick, shown once in the title.
    pub notice: &'a str,
    pub action: &'a str,
    /// List index under the mouse (-1 none); paints only.
    pub hover_row: i32,
    /// Action value under the mouse ('' none); paints only.
    pub hover_value: &'a str,
}

fn footer_hints() -> Vec<Hint> {
    let mut h = vec![
        Hint::plain("w/s", "move"),
        Hint::plain("a/d", "action"),
        Hint::key("enter", "run", "Enter"),
    ];
    h.extend(
        LAUNCH_MODES
            .iter()
            .filter_map(|m| m.hotkey.map(|k| Hint::ch(&k.to_string(), m.name, k))),
    );
    h.push(Hint::ch("/", "filter", '/'));
    h.push(Hint::key("esc", "back", "Escape"));
    h
}

fn title(v: &ProjectView<'_>, g: &Glyphs) -> String {
    let (known, hidden) = project_count(v.projects, v.filter, v.cwd);
    let mut t = format!("project {} {known} known", g.h);
    if hidden > 0 {
        t = format!("{t} {} {hidden} hidden", g.h);
    }
    if !v.filter.is_empty() || v.typing {
        t = format!("{t} {} filter: {}", g.h, clean_text(v.filter));
        if v.typing {
            t.push('_');
        }
    }
    if !v.notice.is_empty() {
        t = format!("{t} {} {}", g.h, clean_text(v.notice));
    }
    t
}

/// One list row's text: project, cwd, or the free-path row.
fn row_text(r: &Row, mark: &str, hovered: bool, inner: usize, wide: bool, v: &ProjectView<'_>, g: &Glyphs) -> String {
    let ascii = v.style.ascii;
    let named = |n: &str| if r.worktree { format!("{} {n}", g.worktree) } else { n.to_owned() };
    let age = r.last_activity_ms.map(|t| relative_age(t, v.now_ms)).unwrap_or_default();
    match r.kind {
        RowKind::Project | RowKind::Cwd => {
            let label = named(&r.name);
            let tail = if wide { r.path.as_str() } else { "" };
            let base = ListRow {
                mark,
                label: &label,
                width: inner,
                ascii,
                hover: hovered,
                ..ListRow::default()
            };
            if r.kind == RowKind::Cwd && age.is_empty() {
                new_list_row(&ListRow {
                    tail,
                    dim_tail: true,
                    path_tail: true,
                    ..base
                })
            } else if wide {
                new_list_row(&ListRow {
                    tail,
                    age: &age,
                    dim_tail: true,
                    dim_age: true,
                    path_tail: true,
                    ..base
                })
            } else {
                new_list_row(&ListRow {
                    age: &age,
                    dim_age: true,
                    age_gutter: true,
                    ..base
                })
            }
        }
        RowKind::Path => {
            let name = limit_line(&theme::strip_ctl(&r.name), inner.saturating_sub(mark.chars().count() + 3), ascii);
            let row = format!("{mark}{} {name}", g.bullet);
            if hovered {
                theme::add_hover_span(&format!("{row}{}", " ".repeat(inner.saturating_sub(display_width(&row)))))
            } else {
                row
            }
        }
    }
}

/// Where the list sits in one frame.
struct Window {
    index: usize,
    start: usize,
    visible: usize,
    inner: usize,
    wide: bool,
}

/// The visible rows and the body line of each. Two separator rules, mirrored by
/// `project_viewport`: a blank line above the free-path row unless it opens the window, and one
/// under the cwd row unless it closes the window or the free-path row follows it.
fn list_body(rows: &[Row], w: &Window, v: &ProjectView<'_>, g: &Glyphs) -> (Vec<String>, Vec<usize>) {
    let hover_row = usize::try_from(v.hover_row).ok();
    let end = w.start + w.visible;
    let mut body: Vec<String> = Vec::new();
    let mut row_ys: Vec<usize> = Vec::new();
    for i in w.start..end {
        let Some(r) = rows.get(i) else { break };
        if r.kind == RowKind::Path && !body.is_empty() {
            body.push(String::new());
        }
        let mark = if i == w.index {
            format!(" {} ", g.cursor)
        } else {
            "   ".to_owned()
        };
        row_ys.push(body.len());
        body.push(row_text(r, &mark, hover_row == Some(i), w.inner, w.wide, v, g));
        if r.kind == RowKind::Cwd && i + 1 < end && rows.get(i + 1).is_some_and(|n| n.kind != RowKind::Path) {
            body.push(String::new());
        }
    }
    (body, row_ys)
}

/// `Get-ProjectFrame`. The current directory leads, the registry follows newest first, the
/// free-path row closes the list; the action field is a radio row under it.
pub(crate) fn project_frame(v: &ProjectView<'_>) -> Frame {
    if v.width < MIN_WIDTH || v.height < MIN_HEIGHT {
        return Frame {
            lines: frame::too_small_frame(v.width, v.height, v.style.ascii),
            map: RowMap {
                list: Some(ListMap::default()),
                ..RowMap::default()
            },
        };
    }
    let g = glyphs(v.style.ascii);
    let fw = frame::frame_width(v.width);
    let rows = project_rows(v.projects, v.filter, v.cwd);
    let footer = new_hint_footer(&footer_hints(), &g, fw, !v.style.color);
    let wide = v.width >= TWO_PANE_WIDTH;
    let detail = usize::from(!wide);
    let body_rows = v.height.saturating_sub(6 + detail + footer.lines.len()).max(3);
    let index = v.index.min(rows.len().saturating_sub(1));
    let list_lines = viewport(rows.len(), index, body_rows).1 + 2;
    let (start, visible) = project_viewport(&rows, index, list_lines);
    let inner = fw.saturating_sub(2);
    let w = Window {
        index,
        start,
        visible,
        inner,
        wide,
    };
    let (mut body, row_ys) = list_body(&rows, &w, v, &g);
    while body.len() < list_lines {
        body.push(String::new());
    }
    if !wide {
        let path = theme::strip_ctl(rows.get(index).map_or("", |r| r.path.as_str()));
        body.push(if path.is_empty() {
            String::new()
        } else {
            format!(
                "   {}{}{}",
                theme::DIM_OPEN,
                limit_path(&path, inner.saturating_sub(4), v.style.ascii),
                theme::DIM_CLOSE
            )
        });
    }

    let values = project_actions();
    let current = step_project_action(v.action, 0);
    let radio = new_radio_row(
        &RadioSpec {
            prefix: "   ",
            label: "action",
            values: &values,
            current: &current,
            texts: &values,
            label_width: 8,
            max_width: inner,
            hover: v.hover_value,
            compact: false,
        },
        &g,
    );
    let action_index = body.len();
    body.push(radio.text);

    let lines = new_box(&body, fw, &title(v, &g), &g, v.style.ascii);
    let first_row_y = lines.len() - body.len() - 1;
    let y = |n: usize| i32::try_from(first_row_y + n).unwrap_or(i32::MAX);
    let mut map = RowMap {
        list: Some(ListMap {
            first_row_y: y(0),
            row_count: i32::try_from(visible).unwrap_or(0),
            start: i32::try_from(start).unwrap_or(0),
            row_ys: row_ys.iter().map(|r| y(*r)).collect(),
        }),
        action: Some(ActionHit {
            y: y(action_index),
            cells: radio
                .cells
                .iter()
                .map(|c| CellSpan {
                    start: c.start + 1,
                    end: c.end + 1,
                    value: c.value.clone(),
                })
                .collect(),
        }),
        ..RowMap::default()
    };
    let selected = launch_mode(&current)
        .and_then(|m| m.hotkey)
        .map(|k| (String::new(), Some(k)))
        .into_iter()
        .collect();
    let light = FooterLight {
        hover: frame::footer_hover(&footer, v.hover),
        selected,
    };
    let painter = |l: &str, enabled: bool| add_picker_color(l, enabled, &g);
    let lines = frame::complete_frame(&lines, &footer, fw, v.style, &painter, &light, &mut map);
    Frame { lines, map }
}

#[cfg(test)]
#[expect(clippy::many_single_char_names, reason = "short fixture names")]
mod tests {
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
            index,
            filter: "",
            typing: false,
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
        }
    }

    fn golden(name: &str) -> String {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wide-frames")
            .join(format!("{name}.txt"));
        std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
    }

    fn text(f: &Frame) -> String {
        f.lines.join("\n")
    }

    fn plain(f: &Frame) -> Vec<String> {
        f.lines.iter().map(|l| theme::remove_ansi_color(l)).collect()
    }

    #[test]
    fn golden_project_100x30() {
        let ps = golden_projects();
        let f = project_frame(&view(&ps, 1, r"C:\Users\sample\here", 100, 30));
        assert_eq!(text(&f), golden("project-100x30"));
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
        assert_eq!(text(&f), golden("project-120x30-hover-color"));
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
            "[t] worktree",
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
        assert_eq!((l.first_row_y, l.row_count, l.start), (1, 16, 16));
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
        assert!(text(&f80).contains("a/d action"));
        assert_eq!(f80.map.footer_lines, 2);
    }

    #[test]
    fn footer_lights_the_selected_action_and_the_hover() {
        let ps = vec![proj("A", r"C:\w\alpha", "alpha")];
        let lit = format!("{}{} r ", theme::ACCENT_BG, theme::ACCENT_FG);
        let color = Style { color: true, ascii: false };
        let r = project_frame(&ProjectView {
            action: "resume",
            style: color,
            ..view(&ps, 0, r"C:\cwd", 80, 24)
        });
        assert!(text(&r).contains(&lit));
        let n = project_frame(&ProjectView {
            style: color,
            ..view(&ps, 0, r"C:\cwd", 80, 24)
        });
        assert!(
            !text(&n).contains(&format!("{}{}", theme::ACCENT_BG, theme::ACCENT_FG)),
            "new lights nothing"
        );
        let c_index = i32::try_from(n.map.footer.iter().position(|s| s.ch == Some('c')).unwrap()).unwrap();
        let h = project_frame(&ProjectView {
            style: color,
            hover: c_index,
            ..view(&ps, 0, r"C:\cwd", 100, 24)
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
}
