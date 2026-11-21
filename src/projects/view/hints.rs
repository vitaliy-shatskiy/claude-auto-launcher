//! The project screen's footer hints and its title line, following the state.

use super::{HomeFocus, ProjectView, clean_text};
use crate::frame::{self, Hint};
use crate::launch::LAUNCH_MODES;
use crate::projects::paths::project_key;
use crate::projects::select::{RowKind, project_count, project_rows};
use crate::theme::{Glyphs, glyphs};

/// The footer follows the state: an open filter names only what works while typing (every
/// button a named key, so a click on one is never typed into the filter).
pub(super) fn footer_hints(v: &ProjectView<'_>) -> Vec<Hint> {
    match v.home.map(|h| h.focus) {
        Some(HomeFocus::Sessions(_)) => {
            // Resume and fork only where the panel lists a session of the selected project.
            let rows = project_rows(v.projects, v.filter, v.cwd);
            let sel = rows.get(v.index.min(rows.len().saturating_sub(1)));
            let listed = v
                .detail
                .is_some_and(|d| !d.sessions.is_empty() && sel.is_some_and(|r| r.kind != RowKind::Path && d.key == project_key(&r.path)));
            let mut h = vec![Hint::plain("w/s", "move").kept()];
            if listed {
                h.push(Hint::key("enter", "resume", "Enter"));
                h.push(Hint::ch("f", "fork", 'f'));
                h.push(Hint::ch("v", "chat", 'v'));
            }
            h.push(Hint::key("tab", "launch", "Tab"));
            h.push(Hint::key("esc", "projects", "Escape"));
            return h;
        }
        Some(HomeFocus::Projects) if !v.typing => {
            let mut h = project_hints(v);
            h.insert(3, Hint::key("tab", "sessions", "Tab"));
            return h;
        }
        Some(HomeFocus::Launch | HomeFocus::Projects) | None => {}
    }
    if v.typing {
        return vec![
            Hint::plain("up/down", "move").kept(),
            Hint::plain("left/right", "action").kept(),
            Hint::key("enter", "run", "Enter"),
            Hint::key("esc", "clear", "Escape"),
        ];
    }
    project_hints(v)
}

/// The list's own keys, the filter closed.
pub(super) fn project_hints(v: &ProjectView<'_>) -> Vec<Hint> {
    let mut h = vec![
        Hint::plain("w/s", "move").kept(),
        Hint::plain(frame::switch_keys(&glyphs(v.style.ascii)), "action").kept(),
        Hint::key("enter", "run", "Enter"),
    ];
    h.extend(
        LAUNCH_MODES
            .iter()
            .filter_map(|m| m.hotkey.map(|k| Hint::ch(&k.to_string(), m.name, k))),
    );
    h.push(Hint::ch("/", "filter", '/'));
    h.push(Hint::ch("p", "pin", 'p'));
    h.push(Hint::ch("h", "hide", 'h'));
    h.push(Hint::ch("v", if v.show_hidden { "back" } else { "hidden" }, 'v'));
    h.push(Hint::key("esc", if v.filter.is_empty() { "back" } else { "clear" }, "Escape"));
    h
}

pub(super) fn title(v: &ProjectView<'_>, g: &Glyphs) -> String {
    let (known, hidden) = project_count(v.projects, v.filter, v.cwd);
    let d = g.dot;
    let filter =
        (!v.filter.is_empty() || v.typing).then(|| format!(" {d} filter: {}{}", clean_text(v.filter), if v.typing { "_" } else { "" }));
    // A rejected pick says so once, in place of the counts: they would push it out of the border.
    if !v.notice.is_empty() {
        return format!("project{} {d} {}", filter.unwrap_or_default(), clean_text(v.notice));
    }
    // The filter leads: a narrow list panel cuts the title's tail, never what is typed.
    let mut t = format!("project{} {d} {known} known", filter.unwrap_or_default());
    let worktrees = v.projects.iter().filter(|p| p.worktree.is_some()).count();
    for (n, what) in [(v.pinned.len(), "pinned"), (hidden, "hidden"), (worktrees, "worktrees")] {
        if n > 0 {
            t = format!("{t} {d} {what} {n}");
        }
    }
    if !v.marked.is_empty() {
        t = format!("{t} {d} {} hidden listed", v.marked.len());
    }
    t
}
