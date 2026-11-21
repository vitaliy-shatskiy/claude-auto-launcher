//! The column beside the project list: the selected project's facts and its sessions, the
//! list panel's width beside them, and the narrow page of the sessions alone.

use super::{Home, HomeFocus, ProjectView, clean_text, relative_age};
use crate::cockpit::{self, Focus, Panel};
use crate::frame::{self, FooterLight, Frame, RowHit, RowMap};
use crate::layout::{display_width, limit_line, limit_path};
use crate::projects::detail::Detail;
use crate::projects::paths::project_key;
use crate::projects::select::{Row, RowKind};
use crate::theme::{self, Glyphs};

/// The selected project's sessions; on the launch page with the sessions focused, the identity
/// and the selected row's fill. A placeholder (`loading…`, `no sessions yet`) is no row to select.
pub(super) fn sessions_panel<'p>(
    lines: Vec<String>,
    d: Option<&Detail>,
    (width, height): (usize, usize),
    home: Option<Home>,
    g: &Glyphs,
) -> Panel<'p> {
    let listed = d.map_or(0, |d| d.sessions.len().min(lines.len()));
    let mut p = Panel::plain("sessions", lines, width, height);
    if let Some(Home {
        focus: HomeFocus::Sessions(i),
        ..
    }) = home
    {
        p.title = cockpit::identity(g);
        "sessions".clone_into(&mut p.page);
        p.focus = Focus::Focused;
        if listed == 0 {
            return p;
        }
        let at = i.min(listed - 1);
        // The cursor mark as on every list: the fill alone is lost without colour.
        if let Some(line) = p.lines.get_mut(at)
            && let Some(rest) = line.strip_prefix("  ")
        {
            *line = format!(" {}{rest}", g.cursor);
        }
        p.selected = Some(at);
    }
    p
}

/// The label column of the selected panel's facts.
const FACT_LABEL: usize = 10;

pub(super) fn fact(label: &str, value: &str) -> String {
    format!("  {}{value}", cockpit::dim(&format!("{label:<FACT_LABEL$}")))
}

/// The cursor's project beside the list: its full path, the git branch, its session count and
/// the newest session's title and age.
pub(super) fn selected_lines(r: Option<&Row>, d: Option<&Detail>, v: &ProjectView<'_>, inner: usize) -> Vec<String> {
    let Some(r) = r else { return Vec::new() };
    let ascii = v.style.ascii;
    if r.kind == RowKind::Path {
        return vec![format!("  {}", cockpit::dim("type a folder path; enter opens it"))];
    }
    let room = inner.saturating_sub(2 + FACT_LABEL + 2);
    let mut out = vec![fact("path", &limit_path(&theme::strip_ctl(&r.path), room, ascii))];
    if r.worktree {
        out.push(fact("worktree", &limit_line(&clean_text(&r.name), room, ascii)));
    }
    let Some(d) = d else {
        out.push(fact(
            "sessions",
            &cockpit::dim(&format!("loading{}", crate::layout::ellipsis(ascii))),
        ));
        return out;
    };
    if let Some(b) = &d.branch {
        out.push(fact("branch", &limit_line(&clean_text(b), room, ascii)));
    }
    out.push(fact("sessions", &d.count.to_string()));
    if let Some(s) = d.sessions.first() {
        // The age leads: right-aligned at the far border it would sit a whole wide column away.
        let age = relative_age(s.modified_ms, v.now_ms);
        let title = limit_line(&clean_text(&s.title), room.saturating_sub(display_width(&age) + 2), ascii);
        out.push(fact("last", &format!("{}  {title}", cockpit::dim(&age))));
    }
    out
}

/// The list panel's width: beside the launch panel the workspace's left column; beside the
/// selected project's facts and sessions the rest of the page, the facts keeping their column (under
/// ~140 columns they give way to `FACTS_MIN`, as on the launch page); alone the whole frame.
pub(super) fn list_panel_width(v: &ProjectView<'_>, fw: usize, wide: bool) -> usize {
    match v.home {
        Some(h) if wide => h.left_width.min(fw),
        _ if wide => {
            fw - cockpit::FACTS_WIDTH
                .min(fw.saturating_sub(LIST_MIN))
                .max(cockpit::FACTS_MIN)
                .min(fw / 2)
        }
        _ => fw,
    }
}

/// The list's width before the facts beside it give way: a name, a path and an age.
const LIST_MIN: usize = 74;

/// The narrow page's sessions: the selected project's sessions panel alone, `fw` by `page_rows`.
pub(super) fn sessions_page(
    v: &ProjectView<'_>,
    sel: Option<&Row>,
    (fw, page_rows): (usize, usize),
    footer: &frame::Footer,
    g: &Glyphs,
) -> Frame {
    let detail = v.detail.filter(|d| sel.is_some_and(|r| d.key == project_key(&r.path)));
    let listed = session_lines(sel, detail, v, fw.saturating_sub(2), page_rows.saturating_sub(2), g);
    let lines = cockpit::render(&sessions_panel(listed, detail, (fw, page_rows), v.home, g), v.style);
    let mut map = RowMap {
        hit_x: RowMap::panel_interior(0, fw),
        side: session_hits(detail, 0, page_rows).and_then(|rows| Some((RowMap::panel_interior(0, fw)?, rows))),
        ..RowMap::default()
    };
    let light = FooterLight {
        hover: frame::footer_hover(footer, v.hover),
        selected: Vec::new(),
    };
    let lines = frame::append_footer(lines, footer, fw, v.style, &light, &mut map);
    Frame { lines, map }
}

/// The sessions panel's lines in the row map (their index there) under the `facts_h` rows of the
/// selected panel; `None` without a listed session.
pub(super) fn session_hits(d: Option<&Detail>, facts_h: usize, sessions_h: usize) -> Option<Vec<RowHit>> {
    let n = d?.sessions.len().min(sessions_h.saturating_sub(2));
    (n > 0).then(|| {
        (0..n)
            .map(|i| RowHit {
                index: i,
                name: "session".to_owned(),
                y: i32::try_from(facts_h + 1 + i).unwrap_or(0),
                cells: Vec::new(),
            })
            .collect()
    })
}

/// The selected project's newest sessions, as many as `rows` hold: title, age, a green dot on a
/// running one.
pub(super) fn session_lines(
    r: Option<&Row>,
    d: Option<&Detail>,
    v: &ProjectView<'_>,
    inner: usize,
    rows: usize,
    g: &Glyphs,
) -> Vec<String> {
    let ascii = v.style.ascii;
    if r.is_none_or(|r| r.kind == RowKind::Path) {
        return Vec::new();
    }
    let Some(d) = d else {
        return vec![format!("  {}", cockpit::dim(&format!("loading{}", crate::layout::ellipsis(ascii))))];
    };
    if d.sessions.is_empty() {
        return vec![format!("  {}", cockpit::dim("no sessions yet"))];
    }
    let shown = d.sessions.iter().take(rows);
    let ages: Vec<String> = shown.clone().map(|s| relative_age(s.modified_ms, v.now_ms)).collect();
    let age_w = ages.iter().map(|a| display_width(a)).max().unwrap_or(0);
    // Ages first, right-aligned in their own column: the titles take the spare width.
    shown
        .zip(&ages)
        .map(|(s, age)| {
            let dot = if s.live {
                theme::tone(theme::TONE_LIVE, &g.on.to_string())
            } else {
                " ".to_owned()
            };
            let title = limit_line(&clean_text(&s.title), inner.saturating_sub(4 + age_w + 2 + 2), ascii);
            format!("  {dot} {}  {title}", cockpit::dim(&format!("{age:>age_w$}")))
        })
        .collect()
}
